//! Test fixtures shared across `apps/api`, `flight-academy-auth`, and
//! `flight-academy-db` integration tests. Consumed only as a
//! `dev-dependency`; never on the runtime dependency graph.
//!
//! Strategy:
//! - **One Postgres container per test binary**, lazily started via a
//!   tokio `OnceCell` on first call. Container shuts down when the binary
//!   exits.
//! - **Fresh database per test**, created inside the shared container with
//!   a UUID-suffixed name. Migrations applied automatically.
//! - The pattern sidesteps `sqlx::test`'s implicit transaction-rollback,
//!   which would interact poorly with the transaction-local role switch
//!   (`set_config('role', 'app_api', true)`) inside `Db::begin_tenant`: the
//!   outer rollback would undo the role change the transaction depends on.
#![allow(clippy::disallowed_methods)] // Test code: runtime SQL for per-test databases, seeding and catalogue checks.

use std::collections::BTreeSet;

use flight_academy_auth::{ActorClass, Role, Subject};
use flight_academy_core::{TenantId, UserId};
use flight_academy_db::{Db, Tenant};
use sqlx::PgPool;
use testcontainers_modules::{
    postgres::Postgres,
    testcontainers::{ContainerAsync, ImageExt, runners::AsyncRunner},
};
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

static CONTAINER: OnceCell<ContainerAsync<Postgres>> = OnceCell::const_new();

/// Serialises the CREATE DATABASE + migration phase of [`fresh_db`].
/// Without this, parallel tests race on the role-creation step of the
/// init migration (roles are cluster-global; the DO-block IF NOT EXISTS
/// check is not atomic with CREATE ROLE). Tests still run their bodies
/// in parallel once `fresh_db` returns.
static MIGRATE_LOCK: Mutex<()> = Mutex::const_new(());

async fn container() -> &'static ContainerAsync<Postgres> {
    CONTAINER
        .get_or_init(|| async {
            // PG 18 matches docker-compose.dev.yml and is what production
            // CNPG can target (ADR-002 §G); the migration set was made
            // PG 17+ portable in 20260605120100 (audit_events triggers
            // switched from FOR EACH ROW to FOR EACH STATEMENT — partitioned
            // tables disallow row triggers on the parent in PG 17+).
            Postgres::default()
                .with_tag("18")
                .start()
                .await
                .expect("failed to start test Postgres container")
        })
        .await
}

async fn admin_dsn() -> String {
    let c = container().await;
    let port = c.get_host_port_ipv4(5432).await.unwrap();
    // `testcontainers-modules` Postgres module defaults: user=postgres,
    // password=postgres, db=postgres.
    format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres")
}

/// Create a fresh database inside the shared container, apply all
/// migrations, and return a `Db` handle. Each call yields an isolated
/// database — tests do not see each other's rows.
///
/// The CREATE DATABASE + migration phase holds [`MIGRATE_LOCK`] to
/// serialise across parallel tests. This is needed because roles
/// (`app_migrator`, `app_api`) are cluster-global in PostgreSQL, and the
/// init migration's `IF NOT EXISTS` check is not atomic with the
/// subsequent `CREATE ROLE` — parallel migrations race, one wins, the
/// rest fail with a duplicate-key violation. Tests run their bodies in
/// parallel once `fresh_db` returns.
pub async fn fresh_db() -> Db {
    let _guard = MIGRATE_LOCK.lock().await;

    let admin = admin_dsn().await;
    let admin_pool = PgPool::connect(&admin).await.expect("connect to admin DB");

    let db_name = format!("test_{}", Uuid::new_v4().simple());
    // CREATE DATABASE cannot be parameterised; identifier is uuid-derived
    // (`test_<hex>` — no quotes, no whitespace), so the only character
    // class that could break out of the quoted identifier doesn't appear.
    // `AssertSqlSafe` is sqlx 0.9's explicit opt-in for dynamic DDL
    // (`sqlx::query` and `raw_sql` require a static SQL string by default
    // to deter accidental injections).
    let create = format!(r#"CREATE DATABASE "{db_name}""#);
    sqlx::raw_sql(sqlx::AssertSqlSafe(create))
        .execute(&admin_pool)
        .await
        .expect("create test DB");

    let test_dsn = {
        let c = container().await;
        let port = c.get_host_port_ipv4(5432).await.unwrap();
        format!("postgres://postgres:postgres@127.0.0.1:{port}/{db_name}")
    };
    let pool = PgPool::connect(&test_dsn)
        .await
        .expect("connect to test DB");
    flight_academy_db::migrator()
        .run(&pool)
        .await
        .expect("run migrations");
    Db::from_pool(pool)
}

/// Seed a tenant row and return the resulting Tenant. Uses the connecting
/// (superuser) session, so tenant_type is whatever the caller supplied —
/// the CHECK constraint will reject invalid values, surfacing as a panic
/// here which is intentional for tests.
pub async fn seed_tenant(db: &Db, slug: &str, name: &str, tenant_type: &str) -> Tenant {
    let id = TenantId::random();
    sqlx::query(
        "INSERT INTO tenants (id, slug, name, tenant_type)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(slug)
    .bind(name)
    .bind(tenant_type)
    .execute(db.pool())
    .await
    .expect("seed tenant");

    db.visible_tenant_by_slug(slug, Some(id))
        .await
        .expect("read seeded tenant")
        .expect("seeded tenant is visible to itself")
}

/// Build a Member-class `Subject` for tests. The richer slots
/// (attributes, elevation) stay at their stub defaults — those slots
/// are not yet read by any policy and exercising them belongs in the
/// PRs that populate them.
pub fn member_subject(tenant_id: TenantId) -> Subject {
    Subject {
        user_id: UserId::random(),
        actor_class: ActorClass::Member,
        tenant_id: Some(tenant_id),
        roles: BTreeSet::new(),
    }
}

/// Build a Member-class `Subject` carrying the `TenantAdmin` role. The
/// caller passes the tenant id the subject is a member-admin of; the
/// role only unlocks administration of *that* tenant via the
/// `TenantAdministration` policy.
pub fn tenant_admin_subject(tenant_id: TenantId) -> Subject {
    let mut roles = BTreeSet::new();
    roles.insert(Role::TenantAdmin);
    Subject {
        user_id: UserId::random(),
        actor_class: ActorClass::Member,
        tenant_id: Some(tenant_id),
        roles,
    }
}

/// Seed N tenant-chain audit events with synthetic payloads. Inserted as
/// the superuser (the test connection's session role), so RLS is
/// bypassed for the seed itself — that is intentional. The point is to
/// give later tenant-scoped reads something to filter against.
pub async fn seed_tenant_audit_events(db: &Db, tenant_id: TenantId, count: usize) {
    for i in 0..count {
        sqlx::query(
            "INSERT INTO audit_events
                (actor_class, chain_kind, chain_id, payload, payload_hash)
             VALUES ('system', 'tenant', $1, $2, $3)",
        )
        .bind(tenant_id)
        .bind(serde_json::json!({ "seed_index": i }))
        .bind(vec![i as u8])
        .execute(db.pool())
        .await
        .expect("seed audit event");
    }
}
