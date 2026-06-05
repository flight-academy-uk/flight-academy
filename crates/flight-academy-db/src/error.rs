//! DB-layer errors. Follows the per-crate Error pattern from
//! `flight-academy-core` (Jeremy Chone / Rust10x — `derive_more::From`,
//! Display-as-Debug). Upstream crates that wrap `Db` calls add their own
//! `From<flight_academy_db::Error>` to lift these into their own enums.

use derive_more::From;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    Sqlx(sqlx::Error),
    Migrate(sqlx::migrate::MigrateError),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}
