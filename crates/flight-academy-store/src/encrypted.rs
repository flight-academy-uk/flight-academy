//! Wrapper types per ADR-001 §D — `EncryptedString` and `EncryptedJson`
//! present an at-rest envelope-encrypted value while letting application
//! code work with plaintext at the API boundary.
//!
//! The v0.1 wrappers expose `seal` and `open` constructors against the
//! [`Envelope`] format. The caller is responsible for resolving the
//! correct DEK per ADR-023 §G — typically via
//! [`KeyProvider::active_dek_for`](crate::key_provider::KeyProvider::active_dek_for)
//! for writes and
//! [`KeyProvider::dek_at_version`](crate::key_provider::KeyProvider::dek_at_version)
//! for reads once the envelope-format bump in C2b.2 lands.
//!
//! The sqlx integration that turns these into transparent column types
//! lands in C2b alongside the first encrypted column.

use crate::aead::{CipherRegistry, Envelope};
use crate::error::StoreResult;
use crate::key_provider::Dek;
use serde::{Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

/// AAD shape per ADR-022 §C — the column-record identity.
///
/// `record_kind` matches the `KeyProvider::for_record` parameter so a
/// single value never gets encrypted with one kind's DEK and decrypted
/// against another's by mistake.
pub struct AadRecord<'a> {
    pub record_kind: &'a str,
    pub record_id: &'a str,
    pub column: &'a str,
}

impl<'a> AadRecord<'a> {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            self.record_kind.len() + 1 + self.record_id.len() + 1 + self.column.len(),
        );
        out.extend_from_slice(self.record_kind.as_bytes());
        out.push(b':');
        out.extend_from_slice(self.record_id.as_bytes());
        out.push(b':');
        out.extend_from_slice(self.column.as_bytes());
        out
    }
}

/// An envelope-encrypted UTF-8 string.
pub struct EncryptedString {
    ciphertext: Vec<u8>,
}

impl EncryptedString {
    /// Encrypt `plaintext` under the registry's default algorithm.
    pub fn seal(
        registry: &CipherRegistry,
        dek: &Dek,
        plaintext: &str,
        aad: &AadRecord<'_>,
    ) -> StoreResult<Self> {
        let cipher = registry.default_cipher();
        let aad_bytes = aad.to_bytes();
        let ct = Envelope::encrypt(cipher, dek.bytes(), plaintext.as_bytes(), &aad_bytes)?;
        Ok(Self { ciphertext: ct })
    }

    /// Decrypt an envelope using the registry to dispatch by algo_id.
    /// `dek_for` resolves the DEK to use for the algorithm — at v0.1
    /// the DEK is the same regardless of algorithm (HKDF derivation
    /// is algorithm-agnostic), but the indirection lets a future
    /// per-algorithm DEK story join without changing the caller. The
    /// returned key bytes are wrapped in [`Zeroizing<Vec<u8>>`] so the
    /// buffer is scrubbed when decryption returns.
    pub fn open(
        registry: &CipherRegistry,
        dek_for: impl FnOnce(u8) -> StoreResult<Zeroizing<Vec<u8>>>,
        ciphertext: &[u8],
        aad: &AadRecord<'_>,
    ) -> StoreResult<String> {
        let aad_bytes = aad.to_bytes();
        let pt = Envelope::decrypt(registry, dek_for, ciphertext, &aad_bytes)?;
        String::from_utf8(pt).map_err(|_| crate::error::StoreError::Decrypt)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn from_bytes(ciphertext: Vec<u8>) -> Self {
        Self { ciphertext }
    }
}

/// An envelope-encrypted, JSON-serialised value.
pub struct EncryptedJson<T> {
    ciphertext: Vec<u8>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned> EncryptedJson<T> {
    pub fn seal(
        registry: &CipherRegistry,
        dek: &Dek,
        plaintext: &T,
        aad: &AadRecord<'_>,
    ) -> StoreResult<Self> {
        let cipher = registry.default_cipher();
        // JSON serialisation of plaintext is in-memory key-equivalent
        // material — wrap in Zeroizing so the bytes scrub when seal
        // returns, even on the error path.
        let json = Zeroizing::new(serde_json::to_vec(plaintext)?);
        let aad_bytes = aad.to_bytes();
        let ct = Envelope::encrypt(cipher, dek.bytes(), &json, &aad_bytes)?;
        Ok(Self {
            ciphertext: ct,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn open(
        registry: &CipherRegistry,
        dek_for: impl FnOnce(u8) -> StoreResult<Zeroizing<Vec<u8>>>,
        ciphertext: &[u8],
        aad: &AadRecord<'_>,
    ) -> StoreResult<T> {
        let aad_bytes = aad.to_bytes();
        // Plaintext after decryption is sensitive — keep it in
        // Zeroizing until it's been parsed; the resulting `T` is the
        // caller's responsibility from there.
        let pt = Zeroizing::new(Envelope::decrypt(
            registry, dek_for, ciphertext, &aad_bytes,
        )?);
        let value: T = serde_json::from_slice(&pt)?;
        Ok(value)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.ciphertext
    }

    pub fn from_bytes(ciphertext: Vec<u8>) -> Self {
        Self {
            ciphertext,
            _marker: std::marker::PhantomData,
        }
    }
}
