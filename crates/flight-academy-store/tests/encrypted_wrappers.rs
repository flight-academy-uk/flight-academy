//! Tests for `EncryptedString` and `EncryptedJson<T>` wrappers — the
//! application-facing entry points per ADR-001 §D.
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
//! - Cross-controller decryption with the wrong DEK fails.

use flight_academy_store::{
    AadRecord, ControllerId, EncryptedJson, EncryptedString, KeyProvider,
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

fn dek_for_test() -> flight_academy_store::Dek {
    let kp = KeyProvider::from_master_bytes([0x99; 32]);
    kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::nil()))
}

#[test]
fn encrypted_string_round_trips() {
    let registry = registry();
    let dek = dek_for_test();
    let aad = fixture_aad();
    let plaintext = "robert@shalders.co.uk";

    let encrypted = EncryptedString::seal(&registry, &dek, plaintext, &aad).unwrap();
    let on_disk = encrypted.as_bytes().to_vec();

    // Simulate a round-trip through storage.
    let _ = EncryptedString::from_bytes(on_disk.clone());

    let recovered = EncryptedString::open(
        &registry,
        |_algo_id| Ok(Zeroizing::new(dek.bytes().to_vec())),
        &on_disk,
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, plaintext);
}

#[test]
fn encrypted_json_round_trips_typed_struct() {
    let registry = registry();
    let dek = dek_for_test();
    let aad = fixture_aad();

    let brand = BrandSettings {
        primary: "oklch(0.7 0.15 240)".into(),
        accent: "oklch(0.6 0.18 30)".into(),
        surface_tint: Some("oklch(0.98 0.01 240)".into()),
    };

    let encrypted = EncryptedJson::seal(&registry, &dek, &brand, &aad).unwrap();
    let on_disk = encrypted.as_bytes().to_vec();

    let recovered: BrandSettings = EncryptedJson::open(
        &registry,
        |_algo_id| Ok(Zeroizing::new(dek.bytes().to_vec())),
        &on_disk,
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, brand);
}

#[test]
fn encrypted_string_empty_plaintext_round_trips() {
    let registry = registry();
    let dek = dek_for_test();
    let aad = fixture_aad();

    let encrypted = EncryptedString::seal(&registry, &dek, "", &aad).unwrap();
    let recovered = EncryptedString::open(
        &registry,
        |_| Ok(Zeroizing::new(dek.bytes().to_vec())),
        encrypted.as_bytes(),
        &aad,
    )
    .unwrap();
    assert_eq!(recovered, "");
}

#[test]
fn encrypted_string_large_plaintext_round_trips() {
    let registry = registry();
    let dek = dek_for_test();
    let aad = fixture_aad();
    let plaintext: String = "a".repeat(32 * 1024);

    let encrypted = EncryptedString::seal(&registry, &dek, &plaintext, &aad).unwrap();
    let recovered = EncryptedString::open(
        &registry,
        |_| Ok(Zeroizing::new(dek.bytes().to_vec())),
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
    let kp = KeyProvider::from_master_bytes([0x99; 32]);
    let dek_a = kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::from_u128(1)));
    let dek_b = kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::from_u128(2)));
    let aad = fixture_aad();

    let encrypted = EncryptedString::seal(&registry, &dek_a, "tenant A secret", &aad).unwrap();
    let err = EncryptedString::open(
        &registry,
        |_| Ok(Zeroizing::new(dek_b.bytes().to_vec())),
        encrypted.as_bytes(),
        &aad,
    )
    .expect_err("cross-controller DEK should fail");
    assert!(matches!(err, flight_academy_store::StoreError::Decrypt));
}
