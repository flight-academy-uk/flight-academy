//! flight-academy-core — shared primitives; canonical `Error` enum +
//! `Result<T>` alias per ADR-005 §C.

mod error;

pub use error::{Error, Result};
