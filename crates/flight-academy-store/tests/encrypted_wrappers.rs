//! Tests for `EncryptedString` and `EncryptedJson<T>` wrappers — the
//! application-facing entry points per ADR-001 §D as refined by
//! ADR-023 §B (dek_version in envelope header).
//!
//! Properties under test:
//!
//! - Round-trip a plaintext string through `seal` → `as_bytes` →
//!   `from_bytes` → `open` and recover the original.
//! - Round-trip a typed struct through `EncryptedJson::seal`/`open` and
//!   recover the original.
//! - Empty plaintext is encryptable (auth tag still provides integrity).
//! - Large plaintext (32 KB) round-trips correctly — proves the
//!   wrappers don't impose a size cap on top of the AEAD's natural one.
//! - Cross-controller decryption with the wrong DEK fails per ADR-012 §A
//!   controller-owner rule.
//! - Wrapper round trips dispatch across DEK versions correctly when
//!   the caller's `dek_for` closure routes by `dek_version` — the
//!   wrapper-layer reflection of ADR-023 §E1's mixed-version property.

use flight_academy_store::{
    AadRecord, ControllerId, EncryptedJson, EncryptedString, InMemoryKeyProvider, KeyProvider,
    aead::{CipherRegistry, algo_id},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct BrandSettings {
    primary: String,
    accent: String,
    surface_tint: Option<String>,
}

fn registry() -> CipherRegistry {
    CipherRegistry::for_test(algo_id::AES_256_GCM_SIV)
}

fn fixture_aad() -> AadRecord<'static> {
    AadRecord {
        record_kind: "tenant_brand",
        record_id: "ten_01HXYZ",
        column: "settings",
    }
}

/// Seed a `KeyProvider` with an active DEK for `(Tenant(nil), "tenant_brand")`
/// and return the resolved DEK + its version. Each test gets a fresh
/// provider so the random DEK bytes are independent.
fn fixture_dek() -> (flight_academy_store::Dek, u32) {
    let kp = InMemoryKeyProvider::from_master_bytes([0x99; 32]);
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    kp.active_dek_for(controller, "tenant_brand").unwrap()
}

#[test]
fn encrypted_string_round_trips() {
    let registry = registry();
    let (dek, dek_version) = fixture_dek();
    let aad = fixture_aad();
    let plaintext = "robert@shalders.co.uk";

    let encrypted = EncryptedString::seal(&registry, &dek, dek_version, plaintext, &aad).unwrap();
    let on_disk = encrypted.as_bytes().to_vec();

    // Simulate a round-trip through storage.
    let _ = EncryptedString::from_bytes(on_disk.clone());

    let recovered = EncryptedString::open(
        &registry,
        |_algo_id, _dek_version| Ok(Zeroizing::new(dek.bytes().to_vec())),
        &on_disk,
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, plaintext);
}

#[test]
fn encrypted_json_round_trips_typed_struct() {
    let registry = registry();
    let (dek, dek_version) = fixture_dek();
    let aad = fixture_aad();

    let brand = BrandSettings {
        primary: "oklch(0.7 0.15 240)".into(),
        accent: "oklch(0.6 0.18 30)".into(),
        surface_tint: Some("oklch(0.98 0.01 240)".into()),
    };

    let encrypted = EncryptedJson::seal(&registry, &dek, dek_version, &brand, &aad).unwrap();
    let on_disk = encrypted.as_bytes().to_vec();

    let recovered: BrandSettings = EncryptedJson::open(
        &registry,
        |_algo_id, _dek_version| Ok(Zeroizing::new(dek.bytes().to_vec())),
        &on_disk,
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, brand);
}

#[test]
fn encrypted_string_empty_plaintext_round_trips() {
    let registry = registry();
    let (dek, dek_version) = fixture_dek();
    let aad = fixture_aad();

    let encrypted = EncryptedString::seal(&registry, &dek, dek_version, "", &aad).unwrap();
    let recovered = EncryptedString::open(
        &registry,
        |_, _| Ok(Zeroizing::new(dek.bytes().to_vec())),
        encrypted.as_bytes(),
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, "");
}

#[test]
fn encrypted_string_large_plaintext_round_trips() {
    let registry = registry();
    let (dek, dek_version) = fixture_dek();
    let aad = fixture_aad();
    let plaintext: String = "a".repeat(32 * 1024);

    let encrypted = EncryptedString::seal(&registry, &dek, dek_version, &plaintext, &aad).unwrap();
    let recovered = EncryptedString::open(
        &registry,
        |_, _| Ok(Zeroizing::new(dek.bytes().to_vec())),
        encrypted.as_bytes(),
        &aad,
    )
    .unwrap();
    assert_eq!(recovered.len(), plaintext.len());
    assert_eq!(recovered, plaintext);
}

#[test]
fn cross_controller_dek_fails_decrypt() {
    // ADR-012 §A: the controller-owner rule means a controller's DEK
    // never decrypts another controller's record. Concretely: if we
    // try to open ciphertext sealed under tenant A's DEK using tenant
    // B's DEK, decryption fails.
    let registry = registry();
    let kp = InMemoryKeyProvider::from_master_bytes([0x99; 32]);
    let controller_a = ControllerId::Tenant(Uuid::from_u128(1));
    let controller_b = ControllerId::Tenant(Uuid::from_u128(2));
    kp.generate_dek(controller_a, "tenant_brand").unwrap();
    kp.generate_dek(controller_b, "tenant_brand").unwrap();
    let (dek_a, ver_a) = kp.active_dek_for(controller_a, "tenant_brand").unwrap();
    let (dek_b, _) = kp.active_dek_for(controller_b, "tenant_brand").unwrap();
    let aad = fixture_aad();

    let encrypted =
        EncryptedString::seal(&registry, &dek_a, ver_a, "tenant A secret", &aad).unwrap();
    let err = EncryptedString::open(
        &registry,
        |_, _| Ok(Zeroizing::new(dek_b.bytes().to_vec())),
        encrypted.as_bytes(),
        &aad,
    )
    .expect_err("cross-controller DEK should fail");
    assert!(matches!(err, flight_academy_store::StoreError::Decrypt));
}

#[test]
fn wrapper_dispatches_across_dek_versions() {
    // ADR-023 §E1 at the wrapper layer: a single column can carry
    // ciphertexts under multiple DEK versions during rotation, and
    // the caller's `dek_for` closure routes to the right key by
    // `(controller, record_kind, dek_version)`. Here a tenant rotates
    // once; both old and new ciphertexts decrypt via the same closure.
    let registry = registry();
    let kp = InMemoryKeyProvider::from_master_bytes([0x99; 32]);
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();

    let (dek_v1, ver_v1) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    let aad = fixture_aad();
    let ct_v1 =
        EncryptedString::seal(&registry, &dek_v1, ver_v1, "first generation", &aad).unwrap();
    let bytes_v1 = ct_v1.as_bytes().to_vec();

    // Rotate to a new active version.
    kp.rotate_dek(controller, "tenant_brand").unwrap();
    let (dek_v2, ver_v2) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(ver_v2, 2);
    let ct_v2 =
        EncryptedString::seal(&registry, &dek_v2, ver_v2, "second generation", &aad).unwrap();
    let bytes_v2 = ct_v2.as_bytes().to_vec();

    // The dek_for closure routes by dek_version — same shape a sqlx-
    // backed reader will use when it calls `KeyProvider::dek_at_version`.
    let dek_for = |_algo_id: u8, requested: u32| {
        let dek = kp
            .dek_at_version(controller, "tenant_brand", requested)
            .unwrap();
        Ok(Zeroizing::new(dek.bytes().to_vec()))
    };

    let pt_v1 = EncryptedString::open(&registry, dek_for, &bytes_v1, &aad).unwrap();
    let pt_v2 = EncryptedString::open(&registry, dek_for, &bytes_v2, &aad).unwrap();
    assert_eq!(pt_v1, "first generation");
    assert_eq!(pt_v2, "second generation");
}
