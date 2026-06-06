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
    /// The pool's session role does not meet the audit chain writer's
    /// invariant (see `audit::verify_pool_role`). The `bypasses_rls`
    /// field is the silent-failure axis: without RLS bypass, the
    /// `prev_hash` SELECT is filtered to nothing and every row becomes a
    /// new "first" entry — the chain forks without surfacing an error
    /// at the write site. INSERT and SELECT grant deficits would
    /// surface as Sqlx errors at first write, but pre-flighting them
    /// at startup turns first-traffic noise into refuse-to-start.
    AuditPoolRoleUnfit {
        role: String,
        can_insert: bool,
        can_select: bool,
        bypasses_rls: bool,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for Error {}
