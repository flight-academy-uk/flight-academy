//! Tests for the `KeyProvider` HKDF derivation surface per ADR-012 §A.
//!
//! Properties under test:
//!
//! - **Determinism**: same `(master, record_kind, controller)` → same DEK
//!   bytes. The encryption call-site relies on this when re-deriving the
//!   key on every request (we do not cache DEKs at v0.1).
//! - **Independence by `record_kind`**: different record kinds under the
//!   same controller yield different DEKs — the HKDF `info` parameter
//!   binds the kind.
//! - **Independence by controller**: different controllers yield
//!   different DEKs even for the same `record_kind` — HKDF `salt` binds
//!   the controller.
//! - **Tenant vs user prefix discrimination**: two controllers with
//!   identical UUID bytes but different kinds (`Tenant` vs `User`)
//!   yield different DEKs.
//! - **Master-key file IO**: reads exactly 32 bytes; rejects shorter/
//!   longer files; rejects missing files.

use flight_academy_store::{ControllerId, KeyProvider, StoreError};
use std::io::Write;
use uuid::Uuid;

fn master() -> [u8; 32] {
    [0x42; 32]
}

#[test]
fn for_record_is_deterministic() {
    let kp = KeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    let a = kp.for_record("tenant_brand", controller);
    let b = kp.for_record("tenant_brand", controller);
    assert_eq!(a.bytes(), b.bytes());
}

#[test]
fn different_record_kinds_derive_different_keys() {
    let kp = KeyProvider::from_master_bytes(master());
    let controller = ControllerId::Tenant(Uuid::nil());
    let a = kp.for_record("tenant_brand", controller);
    let b = kp.for_record("safety_occurrence", controller);
    assert_ne!(a.bytes(), b.bytes());
}

#[test]
fn different_controllers_derive_different_keys() {
    let kp = KeyProvider::from_master_bytes(master());
    let a = kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::nil()));
    let b = kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::from_u128(1)));
    assert_ne!(a.bytes(), b.bytes());
}

#[test]
fn tenant_and_user_with_same_uuid_derive_different_keys() {
    // The kind prefix in the salt bytes must distinguish tenant DEKs
    // from user DEKs even when the underlying UUIDs collide by chance
    // — the two records should never share an encryption key.
    let kp = KeyProvider::from_master_bytes(master());
    let same_uuid = Uuid::from_u128(0xABCDEF);
    let as_tenant = kp.for_record("settings", ControllerId::Tenant(same_uuid));
    let as_user = kp.for_record("settings", ControllerId::User(same_uuid));
    assert_ne!(as_tenant.bytes(), as_user.bytes());
}

#[test]
fn different_master_keys_derive_different_dek() {
    // Sanity: the master KEK is actually load-bearing for the
    // derivation; two different masters must produce different DEKs.
    let a = KeyProvider::from_master_bytes([0x42; 32])
        .for_record("settings", ControllerId::Tenant(Uuid::nil()));
    let b = KeyProvider::from_master_bytes([0x43; 32])
        .for_record("settings", ControllerId::Tenant(Uuid::nil()));
    assert_ne!(a.bytes(), b.bytes());
}

#[test]
fn master_file_loads_exact_32_bytes() {
    let dir = tempdir_for_test();
    let path = dir.path().join("master.key");
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(&[0x55; 32]).unwrap();
    drop(f);

    let kp = KeyProvider::from_master_file(&path).unwrap();
    let dek = kp.for_record("tenant_brand", ControllerId::Tenant(Uuid::nil()));
    // Equivalent to the in-memory path with the same bytes.
    let reference = KeyProvider::from_master_bytes([0x55; 32])
        .for_record("tenant_brand", ControllerId::Tenant(Uuid::nil()));
    assert_eq!(dek.bytes(), reference.bytes());
}

#[test]
fn master_file_rejects_short_file() {
    let dir = tempdir_for_test();
    let path = dir.path().join("short.key");
    std::fs::write(&path, [0x00; 31]).unwrap();
    let err = KeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyLength { got: 31 }));
}

#[test]
fn master_file_rejects_long_file() {
    let dir = tempdir_for_test();
    let path = dir.path().join("long.key");
    std::fs::write(&path, [0x00; 33]).unwrap();
    let err = KeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyLength { got: 33 }));
}

#[test]
fn master_file_rejects_missing_path() {
    let path = std::path::PathBuf::from("/nonexistent/fa/master.key");
    let err = KeyProvider::from_master_file(&path).err().unwrap();
    assert!(matches!(err, StoreError::MasterKeyIo(_)));
}

/// Minimal scratch-dir helper — tests need a writable temp dir but the
/// crate has no `tempfile` dep at v0.1. Uses `CARGO_TARGET_TMPDIR` if
/// present (Cargo provides it during `cargo test`); falls back to
/// `/tmp/fa-store-test-<pid>` otherwise.
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
