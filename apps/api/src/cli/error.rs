//! Binary-side errors. Distinct from the HTTP-layer `ApiError`
//! (`crate::error` in lib.rs's module tree) — that one renders into
//! `application/problem+json`; this one bubbles up to a CLI exit code.
//!
//! `From` for variants that wrap a source error, so `?` works; Display is
//! rendered from Debug because the binary only prints these on exit.

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
    /// A setting combination that cannot start the server.
    #[from(skip)]
    Config {
        reason: &'static str,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}
