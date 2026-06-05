//! Binary-side errors. Distinct from the HTTP-layer `ApiError`
//! (`crate::error` in lib.rs's module tree) — that one renders into
//! `application/problem+json`; this one bubbles up to a CLI exit code.
//!
//! Jeremy Chone / Rust10x pattern: `derive_more::From` for `?` interop,
//! manual Display rendered as Debug (the binary just prints these on
//! shutdown — no per-variant string adds value).

use derive_more::From;

pub type Result<T> = core::result::Result<T, Error>;

// Variant payloads are read indirectly via the Display-as-Debug impl
// below; the dead-code analyser does not count that as a use.
#[allow(dead_code)]
#[derive(Debug, From)]
pub enum Error {
    Io(std::io::Error),
    Db(flight_academy_db::Error),
    Json(serde_json::Error),
    EnvVar {
        name: &'static str,
        source: std::env::VarError,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}

/// Read a required environment variable. Failure carries both the variable
/// name (which `std::env::VarError` does not) and the underlying error.
pub fn env_var(name: &'static str) -> Result<String> {
    std::env::var(name).map_err(|source| Error::EnvVar { name, source })
}
