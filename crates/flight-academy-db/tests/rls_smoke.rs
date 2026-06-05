//! Verifies the audit_events RLS policy directly at the DB layer. Seeds
//! multi-tenant + multi-chain rows as the connecting (superuser) role,
//! then drops to `app_api` via `Db::begin_tenant` and checks that
//! per-tenant queries see only their own chain's rows and that
//! user-chain rows are invisible to tenant-scoped queries.

use flight_academy_db::Db;
use flight_academy_test_support::{fresh_db, seed_tenant_audit_events};
use uuid::Uuid;

async fn count_for(db: &Db, tenant: Uuid) -> i64 {
    let mut tx = db.begin_tenant(tenant).await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(tx.conn())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    count
}

#[tokio::test]
async fn rls_isolates_tenants_and_excludes_user_chain() {
    let db = fresh_db().await;
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();

    seed_tenant_audit_events(&db, tenant_a, 3).await;
    seed_tenant_audit_events(&db, tenant_b, 2).await;

    // User-chain seed row. RLS policy filters chain_kind = 'tenant' so
    // this should be invisible to all tenant-scoped queries below.
    sqlx::query(
        "INSERT INTO audit_events
            (actor_class, chain_kind, chain_id, payload, payload_hash)
         VALUES ('system', 'user', $1, '{}', '\\x00')",
    )
    .bind(Uuid::new_v4())
    .execute(db.pool())
    .await
    .unwrap();

    assert_eq!(
        count_for(&db, tenant_a).await,
        3,
        "tenant A sees its 3 rows"
    );
    assert_eq!(
        count_for(&db, tenant_b).await,
        2,
        "tenant B sees its 2 rows"
    );
    assert_eq!(
        count_for(&db, Uuid::new_v4()).await,
        0,
        "random tenant sees nothing"
    );
}
