//! AES-256-GCM-SIV (RFC 8452) — algo_id `0x01`, the v0.1 default per
//! ADR-022 §A. Nonce-misuse-resistant: accidental nonce reuse with the
//! same key reveals only "these two plaintexts were equal", not the
//! key.

#![allow(deprecated)]

use super::{AeadCipher, algo_id};
use crate::error::{StoreError, StoreResult};
use aead::{Aead, KeyInit, generic_array::GenericArray};
use aes_gcm_siv::Aes256GcmSiv;

pub struct AesGcmSiv256;

impl AeadCipher for AesGcmSiv256 {
    fn algo_id(&self) -> u8 {
        algo_id::AES_256_GCM_SIV
    }

    fn key_size(&self) -> usize {
        32
    }

    fn nonce_size(&self) -> usize {
        12
    }

    fn encrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> StoreResult<Vec<u8>> {
        let cipher = Aes256GcmSiv::new_from_slice(key).map_err(|_| StoreError::Encrypt)?;
        let nonce = GenericArray::from_slice(nonce);
        cipher
            .encrypt(
                nonce,
                aead::Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| StoreError::Encrypt)
    }

    fn decrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> StoreResult<Vec<u8>> {
        let cipher = Aes256GcmSiv::new_from_slice(key).map_err(|_| StoreError::Decrypt)?;
        let nonce = GenericArray::from_slice(nonce);
        cipher
            .decrypt(
                nonce,
                aead::Payload {
                    msg: ciphertext,
                    aad,
                },
            )
            .map_err(|_| StoreError::Decrypt)
    }
}
