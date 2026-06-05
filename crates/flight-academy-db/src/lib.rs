//! flight-academy-db — sqlx access + embedded migrations per ADR-005 §C /
//! ADR-003 §A. RLS-aware repositories land alongside the first domain
//! slice; this crate is the connection-and-migration foundation.
//!
//! Migrations live under `./migrations/`. `sqlx::migrate!` embeds them at
//! compile time so the binary needs no external `sqlx-cli` at runtime
//! (ADR-003 §C — the same binary serves the hosted K8s Job and the
//! self-host install script per ADR-002 §F / §I).

mod error;

pub use error::{Error, Result};

use sqlx::PgPool;

/// Connection pool handle. Constructed via [`Db::connect`], drives
/// migrations via [`Db::migrate`], and exposes the underlying [`PgPool`]
/// for query helpers in downstream crates via [`Db::pool`].
#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Apply all pending migrations from the embedded set
    /// (`crates/flight-academy-db/migrations/`). Idempotent: the
    /// `_sqlx_migrations` table records what has been applied; this is a
    /// no-op when the database is already at the latest version.
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }
}
