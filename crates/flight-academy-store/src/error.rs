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

    /// No active DEK for the requested `(controller, record_kind)`. Per
    /// ADR-023 §C every controller is expected to have exactly one
    /// active DEK per record_kind at all times after the eager generation
    /// at controller create; this error surfaces either a caller bug
    /// (forgot to call `generate_dek` at creation) or a state corruption
    /// (active row missing after rotation/erasure).
    #[display("no active DEK for the requested controller/record_kind")]
    NoActiveDek,

    /// The requested DEK version was never created for this
    /// `(controller, record_kind)`, or has been crypto-shredded. Per
    /// ADR-023 §E a read against a shredded version is the intended
    /// terminal state — the ciphertext under that version is permanently
    /// unreadable.
    #[display("no DEK at version {version} for the requested controller/record_kind")]
    NoSuchDekVersion { version: u32 },

    /// `generate_dek` called when an active DEK already exists for the
    /// `(controller, record_kind)` pair. Rotation must go through
    /// `rotate_dek`, which atomically retires the previous active row
    /// per ADR-023 §E1.
    #[display("active DEK already exists; use rotate_dek for replacement")]
    AlreadyActiveDek,

    /// `shred_dek` called on an active DEK. Per ADR-023 §E the active
    /// DEK is never shredded directly — rotate it to retire it first,
    /// then shred once the sweep is complete and the overlap window has
    /// elapsed.
    #[display("cannot shred an active DEK; rotate to retire it first")]
    CannotShredActiveDek,

    /// The current `KeyProvider` impl does not support the requested
    /// controller kind. At v0.1 the sqlx-backed `SqlxKeyProvider` in
    /// `flight-academy-db` supports `ControllerId::Tenant` only —
    /// `user_dek_wrappings` ships when the `users` table lands in
    /// Slice D auth.
    #[display("controller kind unsupported by this KeyProvider impl: {reason}")]
    UnsupportedController { reason: &'static str },

    /// Underlying storage error (e.g. sqlx call failure) from a
    /// persistence-backed `KeyProvider` impl. Held as a string so the
    /// store crate stays free of a sqlx dependency; richer typing is
    /// available from the impl's owning crate if the caller needs it.
    #[display("storage layer error: {_0}")]
    Storage(String),

    /// JSON serialise/deserialise failure on the EncryptedJson plaintext
    /// path — surfaces before encryption or after decryption.
    #[display("encrypted JSON codec: {_0}")]
    #[from]
    Json(serde_json::Error),
}

impl std::error::Error for StoreError {}

pub type StoreResult<T> = Result<T, StoreError>;
