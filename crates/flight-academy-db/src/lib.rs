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

use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use uuid::Uuid;

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

impl Db {
    pub async fn connect(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    /// Wrap an externally-constructed pool. The test-support crate uses
    /// this to hand `Db` a pool pointing at a fresh per-test database
    /// inside a shared testcontainer.
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
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
    /// Both `SET LOCAL ROLE app_api` and
    /// `SET LOCAL app.current_tenant = '<uuid>'` are issued inside the
    /// transaction so they reset at commit / rollback — safe with the
    /// pooled connection. The role drop is what makes RLS actually apply
    /// (the pool's session role is normally a superuser, which RLS would
    /// otherwise bypass); the GUC is what
    /// `audit_events_tenant_isolation` reads in its USING clause
    /// (migration `20260605120000_audit_events_rls.sql`).
    pub async fn begin_tenant(&self, tenant_id: Uuid) -> Result<TenantTx> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET LOCAL ROLE app_api")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT set_config('app.current_tenant', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
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
