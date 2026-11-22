//! flight-academy-db — sqlx access + embedded migrations per ADR-005 §C /
//! ADR-003 §A. RLS-aware repositories land alongside the first domain
//! slice; this crate is the connection-and-migration foundation.
//!
//! Migrations live under `./migrations/`. `sqlx::migrate!` embeds them at
//! compile time so the binary needs no external `sqlx-cli` at runtime
//! (ADR-003 §C — the same binary serves the hosted K8s Job and the
//! self-host install script per ADR-002 §F / §I).

pub mod audit;
mod dek_wrappings;
mod error;
mod tenants;

pub use audit::AuditEvent;
pub use dek_wrappings::SqlxKeyProvider;
pub use error::{Error, Result};
pub use tenants::Tenant;

// `test-util` exposes `Db::pool()` to tests. Production code must never use
// the raw pool, so an optimised build with the feature on does not compile.
// CI enables it only for debug builds (clippy and tests use --all-features).
#[cfg(all(feature = "test-util", not(debug_assertions)))]
compile_error!("the `test-util` feature exposes Db::pool(); never enable it in a release build");

use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

pub(crate) type Tx = Transaction<'static, Postgres>;

/// Embedded migrator. Returned as a value so `flight-academy-test-support`
/// can drive migrations against a fresh per-test database without needing
/// to invoke `sqlx::migrate!` from outside this crate (the macro is
/// path-relative to where it is invoked).
pub fn migrator() -> sqlx::migrate::Migrator {
    sqlx::migrate!("./migrations")
}

/// Connection pool handle. Constructed via [`Db::connect`], drives
/// migrations via [`Db::migrate`], and exposes the underlying [`PgPool`]
/// for query helpers in downstream crates via [`Db::pool`].
#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

/// Connection-pool limits, set explicitly instead of relying on sqlx's
/// defaults. `statement_timeout` is applied to every connection as a
/// PostgreSQL session setting; `None` leaves the server default (no limit).
#[derive(Clone, Debug)]
pub struct PoolSettings {
    pub max_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_lifetime: Duration,
    pub statement_timeout: Option<Duration>,
}

impl Db {
    pub async fn connect(database_url: &str, settings: &PoolSettings) -> Result<Self> {
        let mut connect = PgConnectOptions::from_str(database_url)?;
        if let Some(timeout) = settings.statement_timeout {
            connect =
                connect.options([("statement_timeout", format!("{}ms", timeout.as_millis()))]);
        }
        let pool = PgPoolOptions::new()
            .max_connections(settings.max_connections)
            .acquire_timeout(settings.acquire_timeout)
            .idle_timeout(settings.idle_timeout)
            .max_lifetime(settings.max_lifetime)
            .connect_with(connect)
            .await?;
        Ok(Self { pool })
    }

    /// Wrap an externally-constructed pool. The test-support crate uses
    /// this to hand `Db` a pool pointing at a fresh per-test database
    /// inside a shared testcontainer.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    /// The underlying pool, for tests that seed data or inspect the
    /// catalogue directly. Not part of the production API: every query the
    /// application runs is a method on `Db`.
    #[cfg(feature = "test-util")]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Begin a transaction for a write that appends to an audit chain.
    ///
    /// The isolation level is stated explicitly as READ COMMITTED rather
    /// than left to the server's `default_transaction_isolation`: the audit
    /// writer serialises appends by locking the chain's head row, and under
    /// REPEATABLE READ or SERIALIZABLE a writer that waited on that lock
    /// would fail with a serialization error once the holder commits,
    /// instead of carrying on from the new tip. A wait on the lock counts
    /// towards the pool's `statement_timeout`.
    ///
    /// The transaction runs on the pool's session role, not `begin_tenant`'s
    /// `app_api`: audit chain writes need it (see the pool-role invariant on
    /// [`Db::write_tenant_audit_event`]).
    pub(crate) async fn begin_audited(&self) -> Result<Tx> {
        Ok(self
            .pool
            .begin_with("BEGIN ISOLATION LEVEL READ COMMITTED")
            .await?)
    }

    /// Apply all pending migrations from the embedded set
    /// (`crates/flight-academy-db/migrations/`). Idempotent: the
    /// `_sqlx_migrations` table records what has been applied; this is a
    /// no-op when the database is already at the latest version.
    pub async fn migrate(&self) -> Result<()> {
        migrator().run(&self.pool).await?;
        Ok(())
    }

    /// Pre-flight the audit chain writer's pool-role invariant against
    /// this pool. Acquires a connection and runs [`audit::verify_pool_role`]
    /// against it — see that function for what is checked and why the
    /// silent-failure mode (RLS without bypass) matters most. Intended
    /// to be called once during the `serve` startup path, after
    /// [`Db::migrate`] and before binding the listener.
    pub async fn verify_audit_pool_role(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        audit::verify_pool_role(&mut conn).await
    }

    /// Begin a transaction with the active tenant context set.
    ///
    /// The role (`app_api`) and `app.current_tenant` are both set with
    /// transaction-local `set_config`, in one statement, so they reset at
    /// commit or rollback and cannot leak to the next user of the pooled
    /// connection. The role is what makes RLS apply; the setting is what the
    /// `audit_events_tenant_isolation` policy reads.
    pub async fn begin_tenant(&self, tenant_id: Uuid) -> Result<TenantTx> {
        let mut tx = self.pool.begin().await?;
        sqlx::query!(
            "SELECT set_config('role', 'app_api', true) AS role,
                    set_config('app.current_tenant', $1, true) AS tenant",
            tenant_id.to_string()
        )
        .fetch_one(&mut *tx)
        .await?;
        Ok(TenantTx { tx })
    }
}

/// Transaction handle scoped to a single tenant. Obtained via
/// [`Db::begin_tenant`]; queries on [`TenantTx::conn`] see only the
/// rows RLS permits for that tenant.
pub struct TenantTx {
    tx: Transaction<'static, Postgres>,
}

impl TenantTx {
    pub fn conn(&mut self) -> &mut PgConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }
}
