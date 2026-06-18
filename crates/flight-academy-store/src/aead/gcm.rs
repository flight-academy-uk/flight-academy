//! AES-256-GCM — algo_id `0x03` per ADR-022 §A. Shipped for operator
//! selection and compatibility with any ciphertext written under
//! ADR-001 §D's original specification. Not the v0.1 default —
//! AES-256-GCM-SIV's nonce-misuse resistance is the load-bearing
//! reason that algorithm was promoted ahead of plain GCM.

#![allow(deprecated)]

use super::{AeadCipher, algo_id};
use crate::error::{StoreError, StoreResult};
use aead::{Aead, KeyInit, generic_array::GenericArray};
use aes_gcm::Aes256Gcm;

pub struct AesGcm256;

impl AeadCipher for AesGcm256 {
    fn algo_id(&self) -> u8 {
        algo_id::AES_256_GCM
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
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| StoreError::Encrypt)?;
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
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| StoreError::Decrypt)?;
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
