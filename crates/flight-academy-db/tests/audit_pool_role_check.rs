//! Verifies the audit pool-role pre-flight (`audit::verify_pool_role`)
//! distinguishes the happy path from the silent-failure case it exists
//! to catch: a pool whose session role is subject to RLS on
//! `audit_events` would write a chain that forks at every insert
//! (because the `prev_hash` SELECT returns nothing under RLS) without
//! surfacing any error to the caller. See the pool-role invariant on
//! `audit::write_tenant_audit_event_in_tx`.

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
    // init migration (NOLOGIN, no BYPASSRLS) — exactly the failure mode
    // the pre-flight exists to catch: grants are present (INSERT +
    // SELECT on audit_events were granted in the init migration), but
    // RLS bypass is not. A pool ever hardened to connect as this role
    // would silently fork the chain on every write.
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
            bypasses_rls,
        } => {
            assert_eq!(role, "app_api");
            assert!(can_insert, "init migration grants INSERT to app_api");
            assert!(can_select, "init migration grants SELECT to app_api");
            assert!(
                !bypasses_rls,
                "app_api must not bypass RLS — that is the whole point of the policy"
            );
        }
        other => panic!("expected AuditPoolRoleUnfit, got {other:?}"),
    }
}
