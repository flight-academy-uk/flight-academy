//! `KeyProvider` trait + in-memory impl per ADR-023 §G — the wrapped
//! DEK lifecycle for envelope encryption.
//!
//! ## Design
//!
//! A DEK exists per `(controller, record_kind, version)` triple. Each
//! DEK is 32 random bytes drawn from `OsRng` at generation time, then
//! wrapped (AEAD-encrypted) under the master KEK and stored. Reads
//! unwrap on demand; the plaintext DEK lives only for the duration of
//! the request and is zeroized on drop.
//!
//! Why wrapped DEKs and not derived DEKs: ADR-001 §D's GDPR Article 17
//! crypto-shred property requires that destroying a small per-controller
//! secret renders the controller's data unrecoverable. A wrapped DEK
//! provides that secret directly — deleting the wrapped row destroys
//! the only path back to the plaintext DEK. Derivation (HKDF from
//! master + public salt) has no such secret to delete and so cannot
//! satisfy the property; the C2a HKDF mechanism this module replaces
//! had that defect.
//!
//! ## Wrap format
//!
//! Each [`WrappedDek`] is `[nonce(12) || ciphertext(32) || auth_tag(16)]`
//! produced by the AES-256-GCM-SIV wrap operation. The master KEK is
//! the cipher key; the random 12-byte nonce is generated per wrap and
//! prepended to the ciphertext for self-contained storage. The wrap
//! AAD per ADR-023 §C is `"dek-wrap:" || controller_kind || ":" ||
//! controller_uuid_bytes || ":" || record_kind || ":" ||
//! dek_version_be_bytes` — the `"dek-wrap:"` prefix prevents an
//! attacker who obtains a wrapped DEK from substituting it as a data
//! ciphertext (different AAD prefix fails authentication).
//!
//! ## Rotation
//!
//! [`KeyProvider::rotate_dek`] atomically retires the active version
//! and inserts a new active version per ADR-023 §E1. Reads under the
//! retired version still work (the wrapping row remains until shredded)
//! so the sweep job from ADR-023 §F can re-encrypt at its own cadence.
//! [`KeyProvider::shred_dek`] removes a retired wrapping row — the
//! caller is responsible for verifying the sweep completed and the
//! 90-day overlap window elapsed (ADR-013 §F precedent).
//!
//! The in-memory impl ([`InMemoryKeyProvider`]) backs unit tests. The
//! sqlx-backed production impl reads `tenant_dek_wrappings` and
//! `user_dek_wrappings` tables and ships in C2b.3.

use crate::aead::{AeadCipher, AesGcmSiv256};
use crate::error::{StoreError, StoreResult};
use rand_core::{OsRng, RngCore};
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Identity of the controlling principal for a record per ADR-012 §A —
/// the "owning controller" whose DEK encrypts the record.
///
/// The variant distinction (`Tenant` vs `User`) is structural in ADR-023 §A:
/// it routes a generation/rotation/shred to the correct
/// `*_dek_wrappings` table. A tenant erasure cascade can never reach
/// user wrappings and vice versa — the cross-controller isolation
/// invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControllerId {
    Tenant(Uuid),
    User(Uuid),
}

impl ControllerId {
    /// Single byte distinguishing controller kind in wrap-AAD bytes.
    /// `b't'` for tenant, `b'u'` for user — chosen to be human-readable
    /// in a hex dump but otherwise arbitrary.
    fn kind_byte(&self) -> u8 {
        match self {
            Self::Tenant(_) => b't',
            Self::User(_) => b'u',
        }
    }

    fn uuid(&self) -> Uuid {
        match self {
            Self::Tenant(u) | Self::User(u) => *u,
        }
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

/// A DEK wrapped (AEAD-encrypted) under the master KEK per ADR-023 §A.
/// Layout: `[nonce(12)] || [ciphertext(32) || auth_tag(16)]` for the v0.1
/// AES-256-GCM-SIV wrap algorithm — 60 bytes total.
///
/// The byte stream is self-contained: the random per-wrap nonce is
/// prepended so unwrap does not need out-of-band state. The wrap
/// algorithm id (`wrap_algo_id` per ADR-023 §A) is carried in the
/// storage row's metadata rather than in this byte stream — at v0.1
/// only `aes-256-gcm-siv` (algo_id `0x01`) wraps DEKs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedDek(pub Vec<u8>);

impl WrappedDek {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

/// The master Key Encryption Key (KEK). Held in memory only; zeroized
/// on drop. The on-disk file path (K8s Secret mount, ESO-decrypted
/// `age` blob, or operator-supplied 32-byte file) is the production
/// source per ADR-001 §D; in-memory bytes are the test source.
#[derive(ZeroizeOnDrop)]
struct MasterKek([u8; 32]);

impl MasterKek {
    fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn from_file(path: impl AsRef<Path>) -> StoreResult<Self> {
        // The file-read buffer is held in `Zeroizing<Vec<u8>>` so it is
        // zeroed on drop regardless of whether the length check passes
        // — a 31-byte rejection still scrubs the 31 bytes from memory.
        let bytes = Zeroizing::new(std::fs::read(path.as_ref())?);
        if bytes.len() != 32 {
            return Err(StoreError::MasterKeyLength { got: bytes.len() });
        }
        let mut master = [0u8; 32];
        master.copy_from_slice(&bytes);
        Ok(Self(master))
    }
}

/// `KeyProvider` per ADR-023 §G. Manages the lifecycle of wrapped DEKs
/// for a set of `(controller, record_kind)` pairs.
///
/// The trait shape is implementation-agnostic: the in-memory variant
/// backs unit tests; a sqlx-backed variant lands in C2b.3 reading the
/// `*_dek_wrappings` tables; future KMS-resident variants (AWS KMS,
/// OpenBao Transit) wrap the same shape behind a different wrap layer.
pub trait KeyProvider: Send + Sync + 'static {
    /// Generate a new random DEK for `(controller, record_kind)`, wrap
    /// it under the master KEK, store it as the active version, and
    /// return that version number.
    ///
    /// Per ADR-023 §C this is called eagerly at controller creation.
    /// If an active DEK already exists for the pair, returns
    /// [`StoreError::AlreadyActiveDek`] — rotation must go through
    /// [`KeyProvider::rotate_dek`] for atomic active-to-retired
    /// transition.
    fn generate_dek(&self, controller: ControllerId, record_kind: &str) -> StoreResult<u32>;

    /// Resolve the active DEK for writes. Returns the plaintext DEK
    /// plus the version under which it is currently active.
    ///
    /// Returns [`StoreError::NoActiveDek`] if no active DEK exists for
    /// the pair — surfacing either a caller bug (no eager generation
    /// at create-time) or a state corruption.
    fn active_dek_for(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(Dek, u32)>;

    /// Resolve a specific DEK version for reads — the version byte is
    /// read from the ciphertext envelope header per ADR-023 §B.
    ///
    /// Returns [`StoreError::NoSuchDekVersion`] if the version was
    /// never generated for this pair, or has been crypto-shredded.
    fn dek_at_version(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<Dek>;

    /// Rotate the active DEK: generate a new active version, retire
    /// the previous active version, atomically. Returns
    /// `(new_version, retired_version)`.
    ///
    /// The retired version's wrapping row remains so reads under it
    /// continue to work until [`KeyProvider::shred_dek`] is called.
    /// Per ADR-023 §E1 the sweep job re-encrypts data from the retired
    /// version to the new active version over time; once the sweep
    /// completes and the overlap window elapses, the retired row is
    /// shredded.
    fn rotate_dek(&self, controller: ControllerId, record_kind: &str) -> StoreResult<(u32, u32)>;

    /// Crypto-shred a retired DEK version (remove the wrapping row).
    ///
    /// Active DEKs are not shreddable — call [`KeyProvider::rotate_dek`]
    /// first to retire the version. Caller is responsible for verifying
    /// the sweep is complete and the 90-day overlap window (ADR-013 §F
    /// precedent) has elapsed before shredding; the trait does not
    /// enforce those preconditions.
    fn shred_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<()>;
}

/// In-memory `KeyProvider` impl backing unit tests. Holds wrappings in
/// a `RwLock<HashMap>`; no schema, no I/O after construction.
///
/// Concurrency: a single `RwLock` serialises mutation across all
/// `(controller, record_kind)` pairs — fine for tests with low write
/// contention. The sqlx-backed production impl uses Postgres
/// transactions per ADR-023 §A's row-level concurrency model.
pub struct InMemoryKeyProvider {
    master: MasterKek,
    wrappings: RwLock<HashMap<WrappingKey, WrappingRow>>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct WrappingKey {
    controller: ControllerId,
    record_kind: String,
    version: u32,
}

#[derive(Debug, Clone)]
struct WrappingRow {
    wrapped: WrappedDek,
    state: WrappingState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WrappingState {
    Active,
    Retired,
}

impl InMemoryKeyProvider {
    /// Construct from in-memory master bytes. Used by tests.
    pub fn from_master_bytes(master: [u8; 32]) -> Self {
        Self {
            master: MasterKek::from_bytes(master),
            wrappings: RwLock::new(HashMap::new()),
        }
    }

    /// Construct from a master-key file. The file must contain exactly
    /// 32 bytes — no leading/trailing whitespace, no length prefix, no
    /// encoding. A K8s Secret mount on `binaryData` produces this shape
    /// directly.
    pub fn from_master_file(path: impl AsRef<Path>) -> StoreResult<Self> {
        Ok(Self {
            master: MasterKek::from_file(path)?,
            wrappings: RwLock::new(HashMap::new()),
        })
    }

    /// Compose the wrap-AAD per ADR-023 §C.
    ///
    /// Format: `"dek-wrap:" || kind_byte || ":" || uuid_bytes(16) ||
    /// ":" || record_kind || ":" || version_be_bytes(4)`. The
    /// `"dek-wrap:"` prefix is the namespace separator — a wrapped DEK
    /// cannot be reused as a data ciphertext because the data AAD does
    /// not carry this prefix (ADR-022 §C).
    fn wrap_aad(controller: ControllerId, record_kind: &str, version: u32) -> Vec<u8> {
        let uuid = controller.uuid();
        let mut aad = Vec::with_capacity(9 + 1 + 1 + 1 + 16 + 1 + record_kind.len() + 1 + 4);
        aad.extend_from_slice(b"dek-wrap:");
        aad.push(controller.kind_byte());
        aad.push(b':');
        aad.extend_from_slice(uuid.as_bytes());
        aad.push(b':');
        aad.extend_from_slice(record_kind.as_bytes());
        aad.push(b':');
        aad.extend_from_slice(&version.to_be_bytes());
        aad
    }

    /// Wrap a 32-byte DEK under the master KEK using AES-256-GCM-SIV.
    /// Output: `[nonce(12) || ciphertext+tag(48)]`.
    fn wrap_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        version: u32,
        dek: &[u8; 32],
    ) -> StoreResult<WrappedDek> {
        let cipher = AesGcmSiv256;
        let mut nonce = vec![0u8; cipher.nonce_size()];
        OsRng.fill_bytes(&mut nonce);
        let aad = Self::wrap_aad(controller, record_kind, version);
        let ct = cipher.encrypt(&self.master.0, &nonce, &aad, dek)?;

        let mut wrapped = Vec::with_capacity(nonce.len() + ct.len());
        wrapped.extend_from_slice(&nonce);
        wrapped.extend_from_slice(&ct);
        // The plaintext DEK bytes the caller passed are still in their
        // own buffer; zeroising that buffer is the caller's
        // responsibility. The local nonce is not key material — but
        // zeroising it costs nothing and keeps the discipline uniform.
        nonce.zeroize();
        Ok(WrappedDek(wrapped))
    }

    /// Unwrap a [`WrappedDek`] under the master KEK. Tag failure
    /// surfaces as [`StoreError::Decrypt`] — indistinguishable from
    /// "wrong KEK", "tampered AAD", or "corrupted wrap bytes" per the
    /// AEAD security contract.
    fn unwrap_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        version: u32,
        wrapped: &WrappedDek,
    ) -> StoreResult<Dek> {
        let cipher = AesGcmSiv256;
        let nonce_size = cipher.nonce_size();
        if wrapped.0.len() < nonce_size {
            return Err(StoreError::Envelope {
                reason: "wrapped DEK shorter than nonce length",
            });
        }
        let (nonce, ct) = wrapped.0.split_at(nonce_size);
        let aad = Self::wrap_aad(controller, record_kind, version);
        let pt = Zeroizing::new(cipher.decrypt(&self.master.0, nonce, &aad, ct)?);
        if pt.len() != 32 {
            return Err(StoreError::Envelope {
                reason: "unwrapped DEK is not 32 bytes",
            });
        }
        let mut dek = [0u8; 32];
        dek.copy_from_slice(&pt);
        Ok(Dek(dek))
    }

    /// Compute the next version number for `(controller, record_kind)`
    /// — `max(existing) + 1`, or `1` if no rows exist.
    fn next_version_for(
        wrappings: &HashMap<WrappingKey, WrappingRow>,
        controller: ControllerId,
        record_kind: &str,
    ) -> u32 {
        wrappings
            .keys()
            .filter(|k| k.controller == controller && k.record_kind == record_kind)
            .map(|k| k.version)
            .max()
            .map_or(1, |max| max + 1)
    }

    /// Find the active version for `(controller, record_kind)`, or
    /// `None` if there isn't one. Per ADR-023 §A's
    /// `one_active_per_*_record_kind` unique index there is at most one.
    fn find_active_version(
        wrappings: &HashMap<WrappingKey, WrappingRow>,
        controller: ControllerId,
        record_kind: &str,
    ) -> Option<u32> {
        wrappings
            .iter()
            .find(|(k, v)| {
                k.controller == controller
                    && k.record_kind == record_kind
                    && v.state == WrappingState::Active
            })
            .map(|(k, _)| k.version)
    }
}

impl KeyProvider for InMemoryKeyProvider {
    fn generate_dek(&self, controller: ControllerId, record_kind: &str) -> StoreResult<u32> {
        // Random 32 bytes from OsRng per ADR-022 §G. Wrapped in
        // Zeroizing so the plaintext DEK bytes scrub when this scope
        // exits, regardless of whether wrap_dek succeeds or fails.
        let mut dek_bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut dek_bytes[..]);

        let mut wrappings = self
            .wrappings
            .write()
            .expect("RwLock poisoned in InMemoryKeyProvider");

        // ADR-023 §A unique index: at most one active row per
        // (controller, record_kind). Generation refuses to overlay an
        // active row; rotation goes through rotate_dek.
        if Self::find_active_version(&wrappings, controller, record_kind).is_some() {
            return Err(StoreError::AlreadyActiveDek);
        }

        let version = Self::next_version_for(&wrappings, controller, record_kind);
        let wrapped = self.wrap_dek(controller, record_kind, version, &dek_bytes)?;

        let key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version,
        };
        wrappings.insert(
            key,
            WrappingRow {
                wrapped,
                state: WrappingState::Active,
            },
        );
        Ok(version)
    }

    fn active_dek_for(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(Dek, u32)> {
        let wrappings = self
            .wrappings
            .read()
            .expect("RwLock poisoned in InMemoryKeyProvider");
        let version = Self::find_active_version(&wrappings, controller, record_kind)
            .ok_or(StoreError::NoActiveDek)?;
        let key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version,
        };
        let row = wrappings.get(&key).ok_or(StoreError::NoActiveDek)?;
        let dek = self.unwrap_dek(controller, record_kind, version, &row.wrapped)?;
        Ok((dek, version))
    }

    fn dek_at_version(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<Dek> {
        let wrappings = self
            .wrappings
            .read()
            .expect("RwLock poisoned in InMemoryKeyProvider");
        let key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version: dek_version,
        };
        let row = wrappings.get(&key).ok_or(StoreError::NoSuchDekVersion {
            version: dek_version,
        })?;
        self.unwrap_dek(controller, record_kind, dek_version, &row.wrapped)
    }

    fn rotate_dek(&self, controller: ControllerId, record_kind: &str) -> StoreResult<(u32, u32)> {
        let mut dek_bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut dek_bytes[..]);

        let mut wrappings = self
            .wrappings
            .write()
            .expect("RwLock poisoned in InMemoryKeyProvider");

        let retired_version = Self::find_active_version(&wrappings, controller, record_kind)
            .ok_or(StoreError::NoActiveDek)?;
        let new_version = Self::next_version_for(&wrappings, controller, record_kind);
        let wrapped = self.wrap_dek(controller, record_kind, new_version, &dek_bytes)?;

        // Atomic: retire the prior active and insert the new active
        // under one write-lock acquisition. In the sqlx impl this same
        // sequence runs inside a transaction.
        let retired_key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version: retired_version,
        };
        wrappings
            .get_mut(&retired_key)
            .expect("active row just resolved")
            .state = WrappingState::Retired;

        let new_key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version: new_version,
        };
        wrappings.insert(
            new_key,
            WrappingRow {
                wrapped,
                state: WrappingState::Active,
            },
        );

        Ok((new_version, retired_version))
    }

    fn shred_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<()> {
        let mut wrappings = self
            .wrappings
            .write()
            .expect("RwLock poisoned in InMemoryKeyProvider");
        let key = WrappingKey {
            controller,
            record_kind: record_kind.to_string(),
            version: dek_version,
        };
        let row = wrappings.get(&key).ok_or(StoreError::NoSuchDekVersion {
            version: dek_version,
        })?;
        if row.state == WrappingState::Active {
            return Err(StoreError::CannotShredActiveDek);
        }
        wrappings.remove(&key);
        Ok(())
    }
}
