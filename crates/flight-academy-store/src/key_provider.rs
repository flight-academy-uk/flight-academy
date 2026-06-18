//! `KeyProvider` trait + a v0.1 in-process impl per ADR-001 §D + ADR-012
//! §A. Resolves a per-`(record_kind, controller)` DEK from a master KEK
//! using HKDF-SHA256.
//!
//! Master-key sourcing modes:
//!
//! - **Test / in-memory**: caller supplies 32 raw bytes via
//!   [`KeyProvider::from_master_bytes`]. Used by unit tests so they do
//!   not pick up an ambient key file.
//! - **Production / hosted**: caller supplies a filesystem path via
//!   [`KeyProvider::from_master_file`]; the file is read at construction
//!   time and the bytes held in memory for the process lifetime. The
//!   file is expected to contain exactly 32 bytes (the K8s Secret mount
//!   pattern: SOPS-encrypted in Git, decrypted by Flux into a Secret,
//!   mounted as a file in the pod).
//!
//! The `age`-encrypted file path described in ADR-001 §D for self-host
//! is not implemented here at v0.1 — it lands in a follow-up alongside
//! a self-host integration. The trait shape does not need to change to
//! add it; only a new constructor.

use crate::error::{StoreError, StoreResult};
use hkdf::Hkdf;
use sha2::Sha256;
use std::path::Path;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Identity of the controlling principal for a record per ADR-012 §A —
/// the "owning controller" whose DEK encrypts the record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerId {
    Tenant(Uuid),
    User(Uuid),
}

impl ControllerId {
    /// Returns a byte representation suitable for use as HKDF salt.
    /// The kind prefix distinguishes tenant-owned from user-owned
    /// DEKs even when the underlying UUIDs collide by chance — the
    /// derived DEKs are independent.
    fn salt_bytes(&self) -> [u8; 17] {
        let mut buf = [0u8; 17];
        match self {
            Self::Tenant(uuid) => {
                buf[0] = b't';
                buf[1..].copy_from_slice(uuid.as_bytes());
            }
            Self::User(uuid) => {
                buf[0] = b'u';
                buf[1..].copy_from_slice(uuid.as_bytes());
            }
        }
        buf
    }
}

/// A 32-byte data encryption key suitable for any of the v0.1 AEADs.
///
/// `Dek` is held in memory only; it is not Serialise/Deserialise (so a
/// log macro or debug print does not leak it) and **zeroes the 32-byte
/// buffer on drop** via the `zeroize` crate — defence in depth on the
/// memory-disclosure threat (core dumps, kernel paging, host memory
/// snapshots).
#[derive(ZeroizeOnDrop)]
pub struct Dek([u8; 32]);

impl Dek {
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Dek {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Dek(<32 bytes, zeroed on drop>)")
    }
}

/// Per ADR-012 §A — derives the right DEK for `(record_kind,
/// controller)` from the master KEK using HKDF-SHA256.
///
/// HKDF parameters:
///
/// | Field | Source |
/// | --- | --- |
/// | `salt` | controller identity bytes (kind prefix + 16 UUID bytes) |
/// | `ikm`  | master KEK (32 bytes) |
/// | `info` | record_kind string (e.g. `"tenant_brand"`, `"user_logbook_entry"`) |
/// | `okm`  | 32 bytes — the per-record DEK |
///
/// Two records of the same `record_kind` under the same controller
/// receive the same DEK — DEKs are per-`(record_kind, controller)`, not
/// per-row, so a single DEK encrypts all rows of one kind for one
/// controller. Per-row uniqueness comes from the AAD binding (ADR-022
/// §C) and the random per-encryption nonce (ADR-022 §G).
#[derive(ZeroizeOnDrop)]
pub struct KeyProvider {
    master: [u8; 32],
}

impl KeyProvider {
    /// Construct from an in-memory 32-byte master key. Used by tests.
    pub fn from_master_bytes(master: [u8; 32]) -> Self {
        Self { master }
    }

    /// Read 32 bytes of master key from `path`. The file must contain
    /// exactly 32 bytes — no leading or trailing whitespace, no
    /// length-prefix, no encoding. A K8s Secret mount on `binaryData`
    /// produces this shape directly.
    ///
    /// The file-read buffer is held in `Zeroizing<Vec<u8>>` so it is
    /// zeroed on drop regardless of whether the length check passes —
    /// a 31-byte rejection still scrubs the 31 bytes from memory.
    pub fn from_master_file(path: impl AsRef<Path>) -> StoreResult<Self> {
        let bytes = Zeroizing::new(std::fs::read(path.as_ref())?);
        if bytes.len() != 32 {
            return Err(StoreError::MasterKeyLength { got: bytes.len() });
        }
        let mut master = [0u8; 32];
        master.copy_from_slice(&bytes);
        Ok(Self { master })
    }

    /// Derive the DEK for a given `(record_kind, controller)` pair.
    ///
    /// The intermediate output-keying-material buffer is constructed
    /// directly inside the returned [`Dek`], whose `ZeroizeOnDrop`
    /// impl scrubs the bytes when the caller drops it.
    pub fn for_record(&self, record_kind: &str, controller: ControllerId) -> Dek {
        let mut salt = controller.salt_bytes();
        let hk = Hkdf::<Sha256>::new(Some(&salt), &self.master);
        let mut dek = Dek([0u8; 32]);
        hk.expand(record_kind.as_bytes(), &mut dek.0)
            .expect("HKDF expand of 32 bytes never fails for SHA-256");
        // Salt carries the controller identity bytes — not secret, but
        // zeroising costs nothing and keeps the discipline uniform.
        salt.zeroize();
        dek
    }
}
