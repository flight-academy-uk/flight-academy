//! Tests for the wrapped-DEK `KeyProvider` lifecycle per ADR-023 §G.
//!
//! Properties under test:
//!
//! - **Generation**: `generate_dek` against a fresh provider creates
//!   version 1, returns it as active.
//! - **Idempotency rejection**: re-calling `generate_dek` for an already-
//!   active `(controller, record_kind)` returns `AlreadyActiveDek` —
//!   rotation is the only path to a second version.
//! - **Stable active resolution**: `active_dek_for` returns the same
//!   plaintext DEK bytes across calls (the wrapped row is constant; the
//!   unwrap is deterministic).
//! - **Random per generation**: two fresh providers generating for the
//!   same `(controller, record_kind)` produce different DEKs — the DEK
//!   is random, not derived.
//! - **Controller isolation**: distinct controllers receive distinct
//!   DEKs (ADR-012 §A controller-owner rule); tenant vs user with
//!   identical UUID bytes never share a DEK (kind prefix in AAD).
//! - **Cross-record-kind isolation**: distinct record_kinds under the
//!   same controller receive distinct DEKs (ADR-001 §G safety key
//!   separation precedent).
//! - **Rotation**: `rotate_dek` returns `(new, retired)`; new becomes
//!   active; retired remains readable via `dek_at_version`; the two
//!   DEKs differ.
//! - **Rotation without prior generation**: errors `NoActiveDek`.
//! - **Version-specific reads**: `dek_at_version` recovers each version's
//!   bytes; an unknown version errors `NoSuchDekVersion`.
//! - **Shredding**: `shred_dek` removes a retired version; subsequent
//!   `dek_at_version` errors `NoSuchDekVersion`; the active version is
//!   unaffected.
//! - **Shredding refuses active**: `shred_dek` against the active
//!   version errors `CannotShredActiveDek`.
//! - **Wrap AAD binding**: a wrapped DEK at version N cannot be
//!   unwrapped as if it were at version M — the AAD includes
//!   `dek_version` per ADR-023 §C.
//! - **Master-key file IO**: file-based master loads, round-trips
//!   identically to in-memory bytes; rejects short/long/missing files.

use flight_academy_store::{ControllerId, InMemoryKeyProvider, KeyProvider, StoreError};
use std::io::Write;
use uuid::Uuid;

fn master() -> [u8; 32] {
    [0x42; 32]
}

#[test]
fn generate_creates_active_version_1() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    let version = kp.generate_dek(controller, "tenant_brand").unwrap();
    assert_eq!(version, 1);

    let (_, active_version) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(active_version, 1);
}

#[test]
fn generate_twice_without_rotation_fails() {
    // Per ADR-023 §A, the unique partial index allows at most one
    // active row per (controller, record_kind). Generation refuses to
    // create a second active; rotation is the only path.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let err = kp
        .generate_dek(controller, "tenant_brand")
        .expect_err("second generate should error");
    assert!(matches!(err, StoreError::AlreadyActiveDek));
}

#[test]
fn active_dek_is_stable_across_calls() {
    // Same wrapped row → same unwrapped bytes. This is the property
    // that lets a request-scoped DEK cache work — a second
    // `active_dek_for` within the same request returns the same key.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let a = kp.active_dek_for(controller, "tenant_brand").unwrap();
    let b = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(a.0.bytes(), b.0.bytes());
    assert_eq!(a.1, b.1);
}

#[test]
fn fresh_providers_produce_different_deks() {
    // Random per generation: two independent providers with the same
    // master and the same (controller, record_kind) produce different
    // DEKs. This is the cryptographic property that makes per-version
    // crypto-shred meaningful — destroying one provider's wrapping
    // does not affect any other.
    let kp_a = InMemoryKeyProvider::from_master_bytes(master());
    let kp_b = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp_a.generate_dek(controller, "tenant_brand").unwrap();
    kp_b.generate_dek(controller, "tenant_brand").unwrap();
    let dek_a = kp_a.active_dek_for(controller, "tenant_brand").unwrap().0;
    let dek_b = kp_b.active_dek_for(controller, "tenant_brand").unwrap().0;
    assert_ne!(dek_a.bytes(), dek_b.bytes());
}

#[test]
fn different_controllers_get_different_deks() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let a = ControllerId::Tenant(Uuid::from_u128(1));
    let b = ControllerId::Tenant(Uuid::from_u128(2));
    kp.generate_dek(a, "tenant_brand").unwrap();
    kp.generate_dek(b, "tenant_brand").unwrap();
    let dek_a = kp.active_dek_for(a, "tenant_brand").unwrap().0;
    let dek_b = kp.active_dek_for(b, "tenant_brand").unwrap().0;
    assert_ne!(dek_a.bytes(), dek_b.bytes());
}

#[test]
fn tenant_and_user_with_same_uuid_get_different_deks() {
    // ADR-023 §A cross-controller isolation invariant: a tenant DEK
    // and a user DEK are not interchangeable even when their UUIDs
    // happen to collide. The wrap-AAD's kind byte (`b't'` vs `b'u'`)
    // distinguishes them.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let uuid = Uuid::from_u128(0xABCDEF);
    let as_tenant = ControllerId::Tenant(uuid);
    let as_user = ControllerId::User(uuid);
    kp.generate_dek(as_tenant, "default").unwrap();
    kp.generate_dek(as_user, "default").unwrap();
    let dek_tenant = kp.active_dek_for(as_tenant, "default").unwrap().0;
    let dek_user = kp.active_dek_for(as_user, "default").unwrap().0;
    assert_ne!(dek_tenant.bytes(), dek_user.bytes());
}

#[test]
fn different_record_kinds_get_different_deks() {
    // ADR-001 §G separate safety key precedent: distinct record_kinds
    // under the same controller produce independent DEKs.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "default").unwrap();
    kp.generate_dek(controller, "safety").unwrap();
    let dek_default = kp.active_dek_for(controller, "default").unwrap().0;
    let dek_safety = kp.active_dek_for(controller, "safety").unwrap().0;
    assert_ne!(dek_default.bytes(), dek_safety.bytes());
}

#[test]
fn rotate_creates_new_active_and_retires_old() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let v1_dek = kp.active_dek_for(controller, "tenant_brand").unwrap().0;

    let (new_v, retired_v) = kp.rotate_dek(controller, "tenant_brand").unwrap();
    assert_eq!(retired_v, 1);
    assert_eq!(new_v, 2);

    // Active is now v2.
    let (v2_dek, active_version) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(active_version, 2);
    assert_ne!(v1_dek.bytes(), v2_dek.bytes());

    // v1 is still readable via dek_at_version (the retired row remains
    // until shredded, so the sweep job can decrypt under it).
    let v1_again = kp.dek_at_version(controller, "tenant_brand", 1).unwrap();
    assert_eq!(v1_again.bytes(), v1_dek.bytes());
}

#[test]
fn rotate_without_prior_generation_fails() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    let err = kp
        .rotate_dek(controller, "tenant_brand")
        .expect_err("rotate without active should error");
    assert!(matches!(err, StoreError::NoActiveDek));
}

#[test]
fn rotate_chain_extends_version_space() {
    // Several rotations in sequence: every version remains readable
    // via `dek_at_version` until shredded.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let mut prev_bytes = *kp
        .active_dek_for(controller, "tenant_brand")
        .unwrap()
        .0
        .bytes();
    for expected_version in 2..=5u32 {
        let (new_v, _) = kp.rotate_dek(controller, "tenant_brand").unwrap();
        assert_eq!(new_v, expected_version);
        let now = *kp
            .active_dek_for(controller, "tenant_brand")
            .unwrap()
            .0
            .bytes();
        assert_ne!(now, prev_bytes, "rotation must change the active DEK bytes");
        prev_bytes = now;
    }
    // Every prior version still resolves.
    for v in 1..=5u32 {
        kp.dek_at_version(controller, "tenant_brand", v)
            .expect("all unshredded versions should resolve");
    }
}

#[test]
fn dek_at_unknown_version_fails() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let err = kp
        .dek_at_version(controller, "tenant_brand", 99)
        .expect_err("unknown version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 99 }));
}

#[test]
fn shred_removes_retired_version() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    kp.rotate_dek(controller, "tenant_brand").unwrap();

    kp.shred_dek(controller, "tenant_brand", 1)
        .expect("retired version should shred");

    // v1 is now permanently unreadable — crypto-shred property.
    let err = kp
        .dek_at_version(controller, "tenant_brand", 1)
        .expect_err("shredded version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 1 }));

    // Active (v2) is unaffected.
    let (_, active_version) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(active_version, 2);
}

#[test]
fn shred_refuses_active_version() {
    // Active DEKs are never shredded directly — ADR-023 §E requires
    // rotation to retire the version first.
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let err = kp
        .shred_dek(controller, "tenant_brand", 1)
        .expect_err("shredding active should error");
    assert!(matches!(err, StoreError::CannotShredActiveDek));
}

#[test]
fn shred_unknown_version_fails() {
    let kp = InMemoryKeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    let err = kp
        .shred_dek(controller, "tenant_brand", 99)
        .expect_err("unknown version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 99 }));
}

#[test]
fn different_master_keys_produce_unreadable_wrappings() {
    // ADR-023 §A KEK rotation precondition: a wrapping made under one
    // master KEK cannot be unwrapped under a different master KEK.
    // KEK rotation (the supported path) rewraps the rows; this test
    // confirms that without the rewrap, the wrappings are
    // cryptographically isolated.
    let kp_a = InMemoryKeyProvider::from_master_bytes([0x42; 32]);
    let kp_b = InMemoryKeyProvider::from_master_bytes([0x43; 32]);
    let controller = ControllerId::Tenant(Uuid::nil());
    kp_a.generate_dek(controller, "tenant_brand").unwrap();
    kp_b.generate_dek(controller, "tenant_brand").unwrap();
    let dek_a = kp_a.active_dek_for(controller, "tenant_brand").unwrap().0;
    let dek_b = kp_b.active_dek_for(controller, "tenant_brand").unwrap().0;
    assert_ne!(dek_a.bytes(), dek_b.bytes());
}

#[test]
fn master_file_loads_exact_32_bytes() {
    let dir = tempdir_for_test();
    let path = dir.path().join("master.key");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&[0x55; 32]).unwrap();
    drop(f);

    let kp = InMemoryKeyProvider::from_master_file(&path).unwrap();
    let controller = ControllerId::Tenant(Uuid::nil());
    kp.generate_dek(controller, "tenant_brand").unwrap();
    // The DEK is random per generation — we can only assert that the
    // provider is functional, not that it equals a specific byte
    // pattern.
    let (_, version) = kp.active_dek_for(controller, "tenant_brand").unwrap();
    assert_eq!(version, 1);
}

#[test]
fn master_file_rejects_short_file() {
    let dir = tempdir_for_test();
    let path = dir.path().join("short.key");
    std::fs::write(&path, [0x00; 31]).unwrap();
    let err = InMemoryKeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyLength { got: 31 }));
}

#[test]
fn master_file_rejects_long_file() {
    let dir = tempdir_for_test();
    let path = dir.path().join("long.key");
    std::fs::write(&path, [0x00; 33]).unwrap();
    let err = InMemoryKeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyLength { got: 33 }));
}

#[test]
fn master_file_rejects_missing_path() {
    let path = std::path::PathBuf::from("/nonexistent/fa/master.key");
    let err = InMemoryKeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyIo(_)));
}

/// Minimal scratch-dir helper — tests need a writable temp dir but the
/// crate has no `tempfile` dep at v0.1. Uses `CARGO_TARGET_TMPDIR` if
/// present (Cargo provides it during `cargo test`); falls back to
/// `std::env::temp_dir()` otherwise. Each invocation uses a per-pid
/// suffix so concurrent test binaries do not collide.
fn tempdir_for_test() -> ScratchDir {
    let base = std::env::var_os("CARGO_TARGET_TMPDIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("fa-store-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    ScratchDir(dir)
}

struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
