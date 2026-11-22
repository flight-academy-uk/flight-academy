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

use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

/// Attempts for a SERIALIZABLE unit before a serialization failure is
/// returned to the caller.
const MAX_SERIALIZABLE_ATTEMPTS: u32 = 3;
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";

pub(crate) type Tx = Transaction<'static, Postgres>;

/// What a transactional unit returns to [`Db::serializable`]: the
/// transaction it was given, handed back for commit, and its result. Taking
/// the transaction by value (rather than borrowing a connection) lets the
/// unit borrow the caller's arguments; boxing it as `Send` lets it run
/// inside axum handlers.
pub(crate) type TxUnit<'a, T> = Pin<Box<dyn Future<Output = (Tx, Result<T>)> + Send + 'a>>;

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

    /// Run `op` in a SERIALIZABLE transaction, retrying the whole unit when
    /// PostgreSQL reports a serialization failure (SQLSTATE 40001), which can
    /// surface at any statement or at commit. Other errors propagate.
    pub(crate) async fn serializable<'a, T>(
        &self,
        what: &'static str,
        tenant_id: Uuid,
        mut unit: impl FnMut(Tx) -> TxUnit<'a, T>,
    ) -> Result<T> {
        for attempt in 1..=MAX_SERIALIZABLE_ATTEMPTS {
            let tx = self
                .pool
                .begin_with("BEGIN ISOLATION LEVEL SERIALIZABLE")
                .await?;
            let (tx, result) = unit(tx).await;
            let outcome = match result {
                Ok(value) => tx.commit().await.map(|()| value).map_err(Error::from),
                Err(err) => Err(err),
            };
            match outcome {
                Err(Error::Sqlx(sqlx::Error::Database(db_err)))
                    if db_err.code().as_deref() == Some(SQLSTATE_SERIALIZATION_FAILURE)
                        && attempt < MAX_SERIALIZABLE_ATTEMPTS =>
                {
                    tracing::warn!(
                        %tenant_id,
                        attempt,
                        max_attempts = MAX_SERIALIZABLE_ATTEMPTS,
                        what,
                        "serialization failure; retrying"
                    );
                }
                other => return other,
            }
        }
        unreachable!("the last attempt always returns");
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
