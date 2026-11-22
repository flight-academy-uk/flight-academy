//! Canonical `Error` enum + `Result<T>` alias per ADR-005 §C.
//!
//! `Display` is rendered from `Debug`. No variant wraps another error, so the
//! enum has no `From` impls; the HTTP layer maps it to RFC 9457
//! problem+json.

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
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
