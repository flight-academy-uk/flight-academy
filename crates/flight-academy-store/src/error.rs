//! Error type for the store crate. Single enum per [feedback_error_style]
//! — `derive_more::From` impls let call sites use `?` without scattered
//! `map_err` calls.

use derive_more::{Display, From};

#[derive(Debug, Display, From)]
pub enum StoreError {
    /// Decryption failed — invalid tag, wrong key, tampered AAD, or
    /// corrupted ciphertext envelope. Deliberately opaque: a decrypt
    /// caller cannot distinguish between "wrong key", "tampered
    /// ciphertext", or "wrong AAD" — all three are authentication
    /// failures with identical security significance.
    #[display("AEAD decrypt failed")]
    Decrypt,

    /// Encryption failed — should be impossible with a well-formed
    /// key + nonce + plaintext, kept for completeness.
    #[display("AEAD encrypt failed")]
    Encrypt,

    /// Ciphertext envelope did not parse — bad version byte, unknown
    /// algorithm id, nonce length outside [12, 32], or truncated.
    #[display("ciphertext envelope malformed: {reason}")]
    Envelope { reason: &'static str },

    /// Algorithm id from a ciphertext header is not registered. Either
    /// the data was written under an algorithm we no longer ship, or
    /// the ciphertext is corrupted.
    #[display("no cipher registered for algo_id {algo_id:#04x}")]
    UnknownAlgorithm { algo_id: u8 },

    /// Configured default-algorithm name from `FA_DEFAULT_AEAD` is not
    /// one of the shipped algorithms.
    #[display("unknown default-AEAD configuration: {name}")]
    UnknownAlgorithmName { name: String },

    /// Master key file at the configured path is missing or unreadable.
    #[display("master key load failed: {_0}")]
    #[from]
    MasterKeyIo(std::io::Error),

    /// Master key bytes are not exactly 32 bytes (256-bit). The on-disk
    /// shape is fixed for v0.1; future rotations may extend.
    #[display("master key must be exactly 32 bytes, got {got}")]
    MasterKeyLength { got: usize },

    /// JSON serialise/deserialise failure on the EncryptedJson plaintext
    /// path — surfaces before encryption or after decryption.
    #[display("encrypted JSON codec: {_0}")]
    #[from]
    Json(serde_json::Error),
}

impl std::error::Error for StoreError {}

pub type StoreResult<T> = Result<T, StoreError>;
