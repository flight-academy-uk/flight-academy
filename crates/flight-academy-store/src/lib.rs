//! `flight-academy-store` — envelope-encryption primitives per ADR-001
//! §D, ADR-012 §A, ADR-022 (pluggable AEAD), and ADR-023 (DEK lifecycle
//! and rotation).
//!
//! Three layers compose:
//!
//! 1. [`aead`] — the [`AeadCipher`] trait plus three implementations
//!    (AES-256-GCM-SIV default, ChaCha20-Poly1305, AES-256-GCM) and the
//!    [`Envelope`] format that frames a ciphertext with its
//!    self-describing algorithm header.
//! 2. [`key_provider`] — the [`KeyProvider`] trait per ADR-023 §G plus
//!    an [`InMemoryKeyProvider`] backing unit tests. Manages the
//!    lifecycle of wrapped DEKs per `(controller, record_kind, version)`
//!    triple — generation, active resolution, version-specific reads,
//!    rotation, and crypto-shredding. The sqlx-backed production impl
//!    reading `tenant_dek_wrappings` / `user_dek_wrappings` tables
//!    ships in C2b.3.
//! 3. [`encrypted`] — [`EncryptedString`] and [`EncryptedJson<T>`]
//!    wrappers present plaintext at the API boundary and ciphertext at
//!    the wire/disk boundary.
//!
//! Object-storage adapters (the MinIO / S3-compatible part of this
//! crate's responsibility per ADR-001 §D) ship in a later slice
//! alongside the first object-storage use case.

pub mod aead;
pub mod encrypted;
pub mod error;
pub mod key_provider;

pub use aead::{AeadCipher, CipherRegistry, Envelope};
pub use encrypted::{AadRecord, EncryptedJson, EncryptedString};
pub use error::{StoreError, StoreResult};
pub use key_provider::{ControllerId, Dek, InMemoryKeyProvider, KeyProvider, WrappedDek};
