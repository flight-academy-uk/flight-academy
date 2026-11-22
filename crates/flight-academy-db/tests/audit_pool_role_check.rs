//! Verifies the audit pool-role pre-flight (`audit::verify_pool_role`)
//! distinguishes the happy path from a pool whose session role cannot
//! drive the audit writer: one subject to RLS, or without the grants on
//! `audit_chain_heads`. See the pool-role invariant on
//! `Db::write_tenant_audit_event`.
#![allow(clippy::disallowed_methods)] // Test code: runtime SQL for per-test databases, seeding and catalogue checks.

use flight_academy_db::{Error, audit};
use flight_academy_test_support::fresh_db;

#[tokio::test]
async fn happy_path_default_pool_role_passes() {
    let db = fresh_db().await;
    let mut conn = db.pool().acquire().await.unwrap();
    audit::verify_pool_role(&mut conn)
        .await
        .expect("test container's superuser pool bypasses RLS");
}

#[tokio::test]
async fn rls_subjected_role_is_flagged_as_unfit() {
    let db = fresh_db().await;
    let mut conn = db.pool().acquire().await.unwrap();

    // Drop to `app_api` on this connection. The role exists from the
    // init migration (NOLOGIN, no BYPASSRLS): it holds INSERT and SELECT
    // on audit_events but nothing on audit_chain_heads, and does not
    // bypass RLS.
    sqlx::query("SET ROLE app_api")
        .execute(&mut *conn)
        .await
        .unwrap();

    let err = audit::verify_pool_role(&mut conn)
        .await
        .expect_err("app_api lacks BYPASSRLS so the check must fail");

    match err {
        Error::AuditPoolRoleUnfit {
            role,
            can_insert,
            can_select,
            can_write_heads,
            bypasses_rls,
        } => {
            assert_eq!(role, "app_api");
            assert!(can_insert, "init migration grants INSERT to app_api");
            assert!(can_select, "init migration grants SELECT to app_api");
            assert!(
                !can_write_heads,
                "app_api holds no privilege on audit_chain_heads"
            );
            assert!(
                !bypasses_rls,
                "app_api must not bypass RLS — that is the whole point of the policy"
            );
        }
        other => panic!("expected AuditPoolRoleUnfit, got {other:?}"),
    }
}
