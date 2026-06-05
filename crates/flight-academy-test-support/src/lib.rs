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
//!   which would interact poorly with the `SET LOCAL ROLE app_api` inside
//!   `Db::begin_tenant` (the outer rollback would undo the role drop the
//!   tx depends on).

use std::collections::BTreeSet;

use flight_academy_auth::{ActorClass, Subject, SubjectAttributes};
use flight_academy_db::Db;
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

/// Build a Member-class `Subject` for tests. The richer slots (roles,
/// attributes, elevation) stay at their stub defaults — those slots are
/// not yet read by any policy and exercising them belongs in the PRs
/// that populate them.
pub fn member_subject(tenant_id: Uuid) -> Subject {
    Subject {
        user_id: Uuid::new_v4(),
        actor_class: ActorClass::Member,
        tenant_id: Some(tenant_id),
        roles: BTreeSet::new(),
        attributes: SubjectAttributes,
        elevation: None,
    }
}

/// Seed N tenant-chain audit events with synthetic payloads. Inserted as
/// the superuser (the test connection's session role), so RLS is
/// bypassed for the seed itself — that is intentional. The point is to
/// give later tenant-scoped reads something to filter against.
pub async fn seed_tenant_audit_events(db: &Db, tenant_id: Uuid, count: usize) {
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
