//! AEAD primitives per [ADR-022](../../../docs/architecture/ADR-022-pluggable-aead.md).
//!
//! Three algorithms ship behind a common [`AeadCipher`] trait, dispatched
//! by a per-ciphertext algorithm-tag byte. The [`Envelope`] type encodes
//! and decodes the on-disk format; the [`CipherRegistry`] maps algo_id
//! bytes to cipher implementations for read-time dispatch.
//!
//! The default for new writes is **AES-256-GCM-SIV** (algo_id `0x01`) per
//! ADR-022 §A. ChaCha20-Poly1305 (`0x02`) and AES-256-GCM (`0x03`) also
//! ship; the active default is operator-selectable via the
//! `FA_DEFAULT_AEAD` environment variable per ADR-022 §E.

// `aead = "0.5"` still exposes `GenericArray::from_slice` as the
// canonical way to construct nonces; the deprecation warning is for a
// future generic-array 0.x → 1.x migration upstream, not actionable in
// this crate.
#![allow(deprecated)]

mod chacha;
mod gcm;
mod gcm_siv;

use crate::error::{StoreError, StoreResult};
use rand_core::{OsRng, RngCore};
use std::collections::HashMap;
use zeroize::Zeroizing;

pub use chacha::ChaCha20Poly1305Aead;
pub use gcm::AesGcm256;
pub use gcm_siv::AesGcmSiv256;

/// Format version byte for the ciphertext envelope per ADR-022 §B.
pub const ENVELOPE_VERSION: u8 = 0x01;

/// Reserved sentinel `algo_id` that is permanently unassignable per
/// ADR-022 §A so a zero-initialised buffer or all-`0xFF` corruption is
/// immediately recognisable.
pub const ALGO_ID_SENTINEL_NEVER: u8 = 0xFF;

/// `algo_id` sentinel reserved per ADR-022 §A so a zero-byte read is
/// unambiguously corruption rather than a valid algorithm.
pub const ALGO_ID_SENTINEL_ZERO: u8 = 0x00;

/// Algorithm IDs assigned in ADR-022 §A.
pub mod algo_id {
    pub const AES_256_GCM_SIV: u8 = 0x01;
    pub const CHACHA20_POLY1305: u8 = 0x02;
    pub const AES_256_GCM: u8 = 0x03;
    // 0x04 reserved for XChaCha20-Poly1305
    // 0x05 reserved for AEGIS-256
    // 0x06 reserved for Ascon-128a
    // 0x07–0xFE reserved for future
}

/// Minimum allowed nonce length in bytes (ADR-022 §B). 12 covers the
/// v0.1 set; the field is bounded `[12, 32]` to admit XChaCha20-Poly1305
/// (24 bytes) and AEGIS-256 (32 bytes) without a format version bump.
pub const MIN_NONCE_LEN: u8 = 12;

/// Maximum allowed nonce length in bytes (ADR-022 §B).
pub const MAX_NONCE_LEN: u8 = 32;

/// One AEAD primitive per ADR-022 §D. Implementations cover the
/// algorithm-specific encrypt/decrypt path; envelope encoding and AAD
/// composition live outside the trait so the impls stay focused on
/// the cryptographic operation.
pub trait AeadCipher: Send + Sync + 'static {
    /// The algorithm tag byte written into the ciphertext header.
    fn algo_id(&self) -> u8;

    /// Key length in bytes. All v0.1 algorithms use 32-byte keys.
    fn key_size(&self) -> usize;

    /// Nonce length in bytes. All v0.1 algorithms use 12-byte nonces;
    /// the [`Envelope`] format carries `nonce_len` explicitly so future
    /// algorithms with longer nonces can join without a format bump.
    fn nonce_size(&self) -> usize;

    /// Encrypt `plaintext` with `key` and `nonce`, binding `aad`.
    /// Returns ciphertext with the authentication tag appended.
    fn encrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> StoreResult<Vec<u8>>;

    /// Decrypt `ciphertext` with `key` and `nonce`, verifying `aad`.
    /// Returns plaintext on tag success. Tag failure returns
    /// [`StoreError::Decrypt`] — the cause (wrong key, tampered AAD,
    /// flipped bits) is deliberately not distinguishable per the
    /// AEAD security contract.
    fn decrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> StoreResult<Vec<u8>>;
}

/// Maps `algo_id` to a registered cipher implementation. Reads look up
/// the cipher by header byte; writes pick the configured default.
pub struct CipherRegistry {
    ciphers: HashMap<u8, Box<dyn AeadCipher>>,
    default_algo_id: u8,
}

impl CipherRegistry {
    /// Construct a registry seeded with all three v0.1 algorithms;
    /// default is the algorithm named in `FA_DEFAULT_AEAD`, falling
    /// back to AES-256-GCM-SIV per ADR-022 §A.
    pub fn with_v0_1_defaults() -> StoreResult<Self> {
        let mut r = Self {
            ciphers: HashMap::new(),
            default_algo_id: algo_id::AES_256_GCM_SIV,
        };
        r.register(Box::new(AesGcmSiv256));
        r.register(Box::new(ChaCha20Poly1305Aead));
        r.register(Box::new(AesGcm256));
        if let Ok(name) = std::env::var("FA_DEFAULT_AEAD") {
            r.default_algo_id = match name.as_str() {
                "aes256-gcm-siv" => algo_id::AES_256_GCM_SIV,
                "chacha20-poly1305" => algo_id::CHACHA20_POLY1305,
                "aes256-gcm" => algo_id::AES_256_GCM,
                other => return Err(StoreError::UnknownAlgorithmName { name: other.into() }),
            };
        }
        Ok(r)
    }

    /// Construct a registry with no environment lookup and an explicit
    /// default — used by tests so they don't pick up an ambient
    /// `FA_DEFAULT_AEAD`.
    pub fn for_test(default_algo_id: u8) -> Self {
        let mut r = Self {
            ciphers: HashMap::new(),
            default_algo_id,
        };
        r.register(Box::new(AesGcmSiv256));
        r.register(Box::new(ChaCha20Poly1305Aead));
        r.register(Box::new(AesGcm256));
        r
    }

    pub fn register(&mut self, cipher: Box<dyn AeadCipher>) {
        self.ciphers.insert(cipher.algo_id(), cipher);
    }

    pub fn default_algo_id(&self) -> u8 {
        self.default_algo_id
    }

    pub fn get(&self, algo_id: u8) -> StoreResult<&dyn AeadCipher> {
        self.ciphers
            .get(&algo_id)
            .map(|b| b.as_ref())
            .ok_or(StoreError::UnknownAlgorithm { algo_id })
    }

    /// Resolve the default cipher (the one used for new writes).
    pub fn default_cipher(&self) -> &dyn AeadCipher {
        // Default algo_id is guaranteed registered by construction.
        self.ciphers
            .get(&self.default_algo_id)
            .map(|b| b.as_ref())
            .expect("default algo_id always registered in constructor")
    }
}

/// The encoded ciphertext envelope per ADR-022 §B.
///
/// Wire format: `[version(1)][algo_id(1)][nonce_len(1)][nonce(nonce_len)][ciphertext+tag]`.
///
/// The header is not inside the AEAD ciphertext, but its bytes are
/// bound to the AEAD via the AAD parameter per ADR-022 §C, so any
/// header tampering causes tag verification to fail.
pub struct Envelope;

impl Envelope {
    /// Encrypt and frame a value into the on-disk envelope.
    ///
    /// `aad_record` is the column-record identity `record_kind:record_id:column`
    /// per ADR-022 §C; the encoder prepends the header bytes
    /// `[version|algo_id|nonce_len]` to it so a header swap fails
    /// authentication.
    pub fn encrypt(
        cipher: &dyn AeadCipher,
        key: &[u8],
        plaintext: &[u8],
        aad_record: &[u8],
    ) -> StoreResult<Vec<u8>> {
        let nonce_len = cipher.nonce_size();
        assert!(
            (MIN_NONCE_LEN as usize..=MAX_NONCE_LEN as usize).contains(&nonce_len),
            "cipher reports nonce_size {nonce_len} outside [12, 32] — algorithm impl bug"
        );

        let mut nonce = vec![0u8; nonce_len];
        OsRng.fill_bytes(&mut nonce);

        let aad = compose_aad(cipher.algo_id(), nonce_len as u8, aad_record);
        let ct = cipher.encrypt(key, &nonce, &aad, plaintext)?;

        let mut out = Vec::with_capacity(3 + nonce_len + ct.len());
        out.push(ENVELOPE_VERSION);
        out.push(cipher.algo_id());
        out.push(nonce_len as u8);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Parse and decrypt an envelope using the registry to dispatch by
    /// the header's `algo_id` byte.
    ///
    /// `key_for` returns the key bytes in a [`Zeroizing<Vec<u8>>`] so
    /// the buffer is scrubbed when the decrypt call returns —
    /// callers cannot accidentally leak key material into long-lived
    /// allocations.
    pub fn decrypt(
        registry: &CipherRegistry,
        key_for: impl FnOnce(u8) -> StoreResult<Zeroizing<Vec<u8>>>,
        envelope: &[u8],
        aad_record: &[u8],
    ) -> StoreResult<Vec<u8>> {
        if envelope.len() < 3 {
            return Err(StoreError::Envelope {
                reason: "envelope shorter than 3-byte header",
            });
        }
        let version = envelope[0];
        let algo_id = envelope[1];
        let nonce_len = envelope[2];

        if version != ENVELOPE_VERSION {
            return Err(StoreError::Envelope {
                reason: "unsupported envelope version",
            });
        }
        if algo_id == ALGO_ID_SENTINEL_ZERO || algo_id == ALGO_ID_SENTINEL_NEVER {
            return Err(StoreError::Envelope {
                reason: "sentinel algo_id is never a real algorithm",
            });
        }
        if !(MIN_NONCE_LEN..=MAX_NONCE_LEN).contains(&nonce_len) {
            return Err(StoreError::Envelope {
                reason: "nonce_len outside [12, 32]",
            });
        }
        let header_end = 3 + nonce_len as usize;
        if envelope.len() < header_end {
            return Err(StoreError::Envelope {
                reason: "envelope truncated before nonce end",
            });
        }

        let nonce = &envelope[3..header_end];
        let ciphertext = &envelope[header_end..];

        let cipher = registry.get(algo_id)?;
        if cipher.nonce_size() != nonce_len as usize {
            return Err(StoreError::Envelope {
                reason: "nonce_len mismatch for declared algorithm",
            });
        }

        let key = key_for(algo_id)?;
        let aad = compose_aad(algo_id, nonce_len, aad_record);
        cipher.decrypt(&key, nonce, &aad, ciphertext)
    }
}

/// Compose the AAD per ADR-022 §C: header bytes folded in front of the
/// column-record identity so a header swap fails authentication.
fn compose_aad(algo_id: u8, nonce_len: u8, aad_record: &[u8]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(3 + aad_record.len());
    aad.push(ENVELOPE_VERSION);
    aad.push(algo_id);
    aad.push(nonce_len);
    aad.extend_from_slice(aad_record);
    aad
}
