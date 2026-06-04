//! Canonical `Error` enum + `Result<T>` alias per ADR-005 §C.
//!
//! Pattern: Jeremy Chone / Rust10x — `derive_more::From` for `?` interop,
//! `Display` rendered as `Debug` (per-variant strings are pointless when the
//! HTTP layer serialises to JSON anyway). The `IntoResponse` impl that turns
//! these into RFC 9457 problem+json lives in the HTTP layer (`apps/api`
//! today; `flight-academy-http-core` when `apps/admin` lands — ADR-005 §F).

use derive_more::From;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    Internal,
    NotFound {
        resource: &'static str,
    },
    Validation {
        field: &'static str,
        message: String,
    },
    Unauthorized,
    Forbidden,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}
