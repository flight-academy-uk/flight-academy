//! flight-academy-core — shared primitives; canonical `Error` enum +
//! `Result<T>` alias per ADR-005 §C; public identifier newtypes per
//! ADR-006 §D.

mod error;
pub mod id;

pub use error::{Error, Result};
pub use id::{IdParseError, TenantId, UserId};
