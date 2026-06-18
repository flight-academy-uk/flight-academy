//! ChaCha20-Poly1305 (RFC 8439) — algo_id `0x02` per ADR-022 §A.
//! Software-fast on architectures without AES-NI; constant-time by
//! construction.

#![allow(deprecated)]

use super::{AeadCipher, algo_id};
use crate::error::{StoreError, StoreResult};
use aead::{Aead, KeyInit, generic_array::GenericArray};
use chacha20poly1305::ChaCha20Poly1305;

pub struct ChaCha20Poly1305Aead;

impl AeadCipher for ChaCha20Poly1305Aead {
    fn algo_id(&self) -> u8 {
        algo_id::CHACHA20_POLY1305
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
        let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| StoreError::Encrypt)?;
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
        let cipher = ChaCha20Poly1305::new_from_slice(key).map_err(|_| StoreError::Decrypt)?;
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
