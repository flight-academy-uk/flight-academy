//! Integration tests for `SqlxKeyProvider` per ADR-023 §G against a
//! real Postgres testcontainer.
//!
//! Properties under test:
//!
//! - **Generation creates version 1** in `tenant_dek_wrappings` and
//!   makes it the active row.
//! - **Generate-twice rejection**: the second `generate_dek` against an
//!   already-active `(tenant, record_kind)` returns
//!   [`StoreError::AlreadyActiveDek`] — both the app-layer check and
//!   the partial unique index `tenant_dek_wrappings_one_active` defend
//!   the invariant.
//! - **Rotation** atomically retires the prior active row and inserts a
//!   new active row in a single transaction; both versions remain
//!   readable until shredded.
//! - **Shred** removes the row (crypto-shred per ADR-001 §D); the
//!   version becomes permanently unreadable.
//! - **Shred refuses active**: the trait-level safety property holds
//!   at the SQL layer too.
//! - **Cross-controller isolation**: distinct tenants get distinct
//!   wrappings and distinct DEKs.
//! - **User-controller rejection**: at v0.1 `ControllerId::User` is
//!   unsupported (the `users` table lands in Slice D); the error is
//!   structured so callers can branch on it.
//! - **ADR-023 §A invariant — tenant erasure cascades**: `DELETE FROM
//!   tenants WHERE id = $1` cascades to `tenant_dek_wrappings` and
//!   shreds every wrapping row for that tenant atomically. This is
//!   the load-bearing privacy property the schema-level cascade
//!   enforces.
//! - **State invariant**: `state = 'active'` implies `retired_at IS NULL`
//!   and `state = 'retired'` implies `retired_at IS NOT NULL` — the
//!   `tenant_dek_wrappings_state_consistency` CHECK rejects violations.

use flight_academy_db::SqlxKeyProvider;
use flight_academy_store::{ControllerId, KeyProvider, MasterKek, StoreError};
use flight_academy_test_support::{fresh_db, seed_tenant};
use uuid::Uuid;

fn master() -> MasterKek {
    MasterKek::from_bytes([0x42; 32])
}

#[tokio::test]
async fn generate_creates_active_version_1() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-test", "ATO Test", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    let version = kp
        .generate_dek(ControllerId::Tenant(tenant.id), "tenant_brand")
        .await
        .unwrap();
    assert_eq!(version, 1);

    let (_, active_version) = kp
        .active_dek_for(ControllerId::Tenant(tenant.id), "tenant_brand")
        .await
        .unwrap();
    assert_eq!(active_version, 1);
}

#[tokio::test]
async fn generate_twice_without_rotation_fails() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-twice", "ATO Twice", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();
    let err = kp
        .generate_dek(controller, "tenant_brand")
        .await
        .expect_err("second generate should error");
    assert!(matches!(err, StoreError::AlreadyActiveDek));
}

#[tokio::test]
async fn rotate_creates_new_active_and_retires_old() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-rotate", "ATO Rotate", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();
    let v1_dek = kp
        .active_dek_for(controller, "tenant_brand")
        .await
        .unwrap()
        .0;

    let (new_v, retired_v) = kp.rotate_dek(controller, "tenant_brand").await.unwrap();
    assert_eq!(retired_v, 1);
    assert_eq!(new_v, 2);

    let (v2_dek, active_version) = kp.active_dek_for(controller, "tenant_brand").await.unwrap();
    assert_eq!(active_version, 2);
    assert_ne!(v1_dek.bytes(), v2_dek.bytes());

    // v1 still readable via dek_at_version.
    let v1_again = kp
        .dek_at_version(controller, "tenant_brand", 1)
        .await
        .unwrap();
    assert_eq!(v1_again.bytes(), v1_dek.bytes());

    // SQL-layer state invariant: v1 is 'retired' with retired_at set;
    // v2 is 'active' with retired_at NULL.
    let rows: Vec<(i32, String, Option<sqlx::types::time::OffsetDateTime>)> = sqlx::query_as(
        "SELECT dek_version, state, retired_at
           FROM tenant_dek_wrappings
          WHERE tenant_id = $1
       ORDER BY dek_version",
    )
    .bind(tenant.id)
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, 1);
    assert_eq!(rows[0].1, "retired");
    assert!(rows[0].2.is_some());
    assert_eq!(rows[1].0, 2);
    assert_eq!(rows[1].1, "active");
    assert!(rows[1].2.is_none());
}

#[tokio::test]
async fn rotate_without_prior_generation_fails() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-norotate", "ATO NoRotate", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    let err = kp
        .rotate_dek(ControllerId::Tenant(tenant.id), "tenant_brand")
        .await
        .expect_err("rotate without active should error");
    assert!(matches!(err, StoreError::NoActiveDek));
}

#[tokio::test]
async fn shred_removes_retired_version() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-shred", "ATO Shred", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();
    kp.rotate_dek(controller, "tenant_brand").await.unwrap();

    kp.shred_dek(controller, "tenant_brand", 1)
        .await
        .expect("retired version should shred");

    // v1 is now permanently unreadable — crypto-shred per ADR-001 §D.
    let err = kp
        .dek_at_version(controller, "tenant_brand", 1)
        .await
        .expect_err("shredded version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 1 }));

    // The row is gone from the table (not soft-deleted).
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tenant_dek_wrappings
          WHERE tenant_id = $1 AND dek_version = 1",
    )
    .bind(tenant.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(count, 0);

    // Active (v2) is unaffected.
    let (_, active_version) = kp.active_dek_for(controller, "tenant_brand").await.unwrap();
    assert_eq!(active_version, 2);
}

#[tokio::test]
async fn shred_refuses_active_version() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-shred-active", "ATO ShredActive", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();
    let err = kp
        .shred_dek(controller, "tenant_brand", 1)
        .await
        .expect_err("shredding active should error");
    assert!(matches!(err, StoreError::CannotShredActiveDek));
}

#[tokio::test]
async fn cross_tenant_dek_isolation() {
    // ADR-012 §A controller-owner rule at the SQL layer: two distinct
    // tenants get distinct wrappings and distinct DEKs.
    let db = fresh_db().await;
    let tenant_a = seed_tenant(&db, "ato-iso-a", "Iso A", "ato").await;
    let tenant_b = seed_tenant(&db, "ato-iso-b", "Iso B", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    kp.generate_dek(ControllerId::Tenant(tenant_a.id), "tenant_brand")
        .await
        .unwrap();
    kp.generate_dek(ControllerId::Tenant(tenant_b.id), "tenant_brand")
        .await
        .unwrap();
    let dek_a = kp
        .active_dek_for(ControllerId::Tenant(tenant_a.id), "tenant_brand")
        .await
        .unwrap()
        .0;
    let dek_b = kp
        .active_dek_for(ControllerId::Tenant(tenant_b.id), "tenant_brand")
        .await
        .unwrap()
        .0;
    assert_ne!(dek_a.bytes(), dek_b.bytes());
}

#[tokio::test]
async fn user_controller_kind_rejected_with_structured_error() {
    let db = fresh_db().await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    let err = kp
        .generate_dek(ControllerId::User(Uuid::nil()), "default")
        .await
        .expect_err("user controller should error at v0.1");
    assert!(matches!(err, StoreError::UnsupportedController { .. }));
}

#[tokio::test]
async fn tenant_erasure_cascades_to_dek_wrappings() {
    // ADR-023 §A's load-bearing privacy invariant: hard-deleting a
    // tenants row cascades to every tenant_dek_wrappings row for that
    // tenant. Verifies crypto-shred works as a single transaction
    // rather than requiring application-layer enumeration.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-erase", "ATO Erase", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    // Seed multiple versions across two record_kinds so the cascade
    // has something to shred.
    kp.generate_dek(controller, "tenant_brand").await.unwrap();
    kp.rotate_dek(controller, "tenant_brand").await.unwrap();
    kp.generate_dek(controller, "safety").await.unwrap();

    let before_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tenant_dek_wrappings WHERE tenant_id = $1")
            .bind(tenant.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(before_count, 3);

    // Hard-delete the tenant row — the erasure ceremony per ADR-013
    // §H. ON DELETE CASCADE shreds every DEK wrapping in one operation.
    sqlx::query("DELETE FROM tenants WHERE id = $1")
        .bind(tenant.id)
        .execute(db.pool())
        .await
        .unwrap();

    let after_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tenant_dek_wrappings WHERE tenant_id = $1")
            .bind(tenant.id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(
        after_count, 0,
        "tenant erasure must cascade to dek_wrappings"
    );

    // Reads now fail with NoActiveDek (the DEKs are gone, the data
    // would be unrecoverable if any existed).
    let err = kp
        .active_dek_for(controller, "tenant_brand")
        .await
        .expect_err("post-erasure active read should fail");
    assert!(matches!(err, StoreError::NoActiveDek));
}

#[tokio::test]
async fn state_invariant_rejects_active_with_retired_at() {
    // The CHECK constraint tenant_dek_wrappings_state_consistency
    // rules out the mid-state "marked active but with retired_at
    // already set". Verifies the schema-level guard, not the impl.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-check", "ATO Check", "ato").await;

    let err = sqlx::query(
        "INSERT INTO tenant_dek_wrappings
            (tenant_id, record_kind, dek_version, wrapped_bytes,
             wrap_algo_id, kek_id, state, retired_at)
         VALUES ($1, 'default', 1, '\\x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000', 1, 'master:v1', 'active', now())",
    )
    .bind(tenant.id)
    .execute(db.pool())
    .await
    .expect_err("active + retired_at set must violate CHECK");
    let s = err.to_string();
    assert!(
        s.contains("tenant_dek_wrappings_state_consistency"),
        "expected state-consistency CHECK violation, got: {s}"
    );
}

#[tokio::test]
async fn fk_to_tenants_rejects_unknown_tenant() {
    let db = fresh_db().await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    // A tenant id that doesn't exist in tenants(id) must be rejected
    // by the FK constraint, surfaced via the storage error variant.
    let err = kp
        .generate_dek(ControllerId::Tenant(Uuid::new_v4()), "tenant_brand")
        .await
        .expect_err("unknown tenant id must violate FK");
    assert!(
        matches!(&err, StoreError::Storage(msg) if msg.contains("foreign key") || msg.contains("23503")),
        "expected FK violation, got: {err:?}"
    );
}

// ---- Unhappy-path tests for read surfaces against missing rows. ----
// These mirror the InMemoryKeyProvider unhappy-path tests so the two
// impls agree on the trait-level error surface a caller sees.

#[tokio::test]
async fn active_dek_for_uninitialised_pair_returns_no_active_dek() {
    // A (tenant, record_kind) that has never had `generate_dek` called
    // returns NoActiveDek, not a Storage error or a panic. This is the
    // shape callers branch on to decide whether to seed the controller
    // before retrying.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-fresh", "ATO Fresh", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    let err = kp
        .active_dek_for(ControllerId::Tenant(tenant.id), "tenant_brand")
        .await
        .expect_err("fresh (tenant, record_kind) should error");
    assert!(matches!(err, StoreError::NoActiveDek));
}

#[tokio::test]
async fn dek_at_unknown_version_returns_no_such_version() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-noversion", "ATO NoVersion", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();

    // Version 99 has never been generated.
    let err = kp
        .dek_at_version(controller, "tenant_brand", 99)
        .await
        .expect_err("unknown version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 99 }));
}

#[tokio::test]
async fn shred_unknown_version_returns_no_such_version() {
    // Distinguishable from CannotShredActiveDek — operator
    // tooling needs to know whether the version was active, retired,
    // or never existed.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-noshred", "ATO NoShred", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();

    let err = kp
        .shred_dek(controller, "tenant_brand", 99)
        .await
        .expect_err("unknown version should error");
    assert!(matches!(err, StoreError::NoSuchDekVersion { version: 99 }));
}

// ---- Cross-controller / cross-record-kind isolation at the SQL layer.

#[tokio::test]
async fn tenant_b_cannot_observe_tenant_a_active_dek() {
    // ADR-012 §A controller-owner rule, SQL-layer assertion: a tenant
    // querying for the active DEK of a (record_kind) it has never
    // generated sees `NoActiveDek` — not silently inherit another
    // tenant's wrapping. This is the SQL-level counterpart to
    // `cross_tenant_dek_isolation` (which only verifies the bytes
    // differ; this verifies absence-of-leakage).
    let db = fresh_db().await;
    let tenant_a = seed_tenant(&db, "ato-leak-a", "Leak A", "ato").await;
    let tenant_b = seed_tenant(&db, "ato-leak-b", "Leak B", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());

    kp.generate_dek(ControllerId::Tenant(tenant_a.id), "tenant_brand")
        .await
        .unwrap();

    let err = kp
        .active_dek_for(ControllerId::Tenant(tenant_b.id), "tenant_brand")
        .await
        .expect_err("tenant B should not see tenant A's wrapping");
    assert!(matches!(err, StoreError::NoActiveDek));
}

#[tokio::test]
async fn cross_record_kind_isolation_at_sql_layer() {
    // ADR-001 §G separate safety key precedent at the SQL layer: a
    // wrapping under record_kind "default" does not surface for a
    // lookup under "safety", even for the same controller. Mirrors
    // the InMemoryKeyProvider test of the same property, ensuring the
    // SQL impl uses record_kind in its WHERE clauses correctly.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-recordkind", "ATO RecordKind", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "default").await.unwrap();

    let err = kp
        .active_dek_for(controller, "safety")
        .await
        .expect_err("safety lookup should not see default wrapping");
    assert!(matches!(err, StoreError::NoActiveDek));
}

// ---- Corruption defence: tampered wrapped_bytes surface as Decrypt.

#[tokio::test]
async fn corrupted_wrapped_bytes_in_db_surface_as_decrypt_failure() {
    // If the wrapping row in the DB is tampered with (a row-level
    // edit by an attacker with DB write access, a corrupted backup
    // restore, etc.), the unwrap operation must fail the AEAD tag
    // check rather than return spurious bytes. Surfaces as
    // `StoreError::Decrypt` per the AEAD contract (ADR-022 §C).
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-tamper", "ATO Tamper", "ato").await;
    let kp = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    kp.generate_dek(controller, "tenant_brand").await.unwrap();

    // Flip a byte in the middle of the wrapped_bytes column. The
    // expression `set byte 30 to (current XOR 0x01)` is portable PG;
    // no superuser tools needed.
    sqlx::query(
        "UPDATE tenant_dek_wrappings
            SET wrapped_bytes = set_byte(wrapped_bytes, 30, get_byte(wrapped_bytes, 30) # 1)
          WHERE tenant_id = $1 AND record_kind = 'tenant_brand'",
    )
    .bind(tenant.id)
    .execute(db.pool())
    .await
    .unwrap();

    let err = kp
        .active_dek_for(controller, "tenant_brand")
        .await
        .expect_err("tampered wrapping must fail decrypt");
    assert!(matches!(err, StoreError::Decrypt));
}

// ---- Inverse state-consistency CHECK for symmetry.

#[tokio::test]
async fn state_invariant_rejects_retired_without_retired_at() {
    // Symmetric to `state_invariant_rejects_active_with_retired_at`:
    // a row marked 'retired' must carry a retired_at timestamp. The
    // CHECK constraint rules out the "marked retired but timestamp
    // missing" mid-state that an interrupted retirement UPDATE could
    // otherwise leave behind.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-symcheck", "ATO SymCheck", "ato").await;

    let err = sqlx::query(
        "INSERT INTO tenant_dek_wrappings
            (tenant_id, record_kind, dek_version, wrapped_bytes,
             wrap_algo_id, kek_id, state, retired_at)
         VALUES ($1, 'default', 1, '\\x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000', 1, 'master:v1', 'retired', NULL)",
    )
    .bind(tenant.id)
    .execute(db.pool())
    .await
    .expect_err("retired + retired_at NULL must violate CHECK");
    let s = err.to_string();
    assert!(
        s.contains("tenant_dek_wrappings_state_consistency"),
        "expected state-consistency CHECK violation, got: {s}"
    );
}

// ---- Concurrent generate: the partial unique index is the structural defence.

#[tokio::test]
async fn concurrent_generate_race_loses_to_partial_unique_index() {
    // Two concurrent `generate_dek` calls on the same
    // (tenant, record_kind) — one wins by COMMIT-order, the other
    // sees either the app-layer `AlreadyActiveDek` check (if it
    // observes the committed row) or the partial unique index
    // violation surfaced as a Storage error. Both outcomes are
    // acceptable; the post-condition is exactly one active row.
    //
    // This documents the race contract: the partial unique index
    // `tenant_dek_wrappings_one_active` is the load-bearing
    // structural guard. The app-layer EXISTS check is a usability
    // niceness (returns a clean AlreadyActiveDek when it fires) but
    // is not the safety mechanism — the index is.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "ato-race", "ATO Race", "ato").await;
    let kp_a = SqlxKeyProvider::new(db.pool().clone(), master());
    let kp_b = SqlxKeyProvider::new(db.pool().clone(), master());
    let controller = ControllerId::Tenant(tenant.id);

    let (a, b) = tokio::join!(
        kp_a.generate_dek(controller, "tenant_brand"),
        kp_b.generate_dek(controller, "tenant_brand"),
    );

    let winners = [a.is_ok(), b.is_ok()];
    assert_eq!(
        winners.iter().filter(|&&x| x).count(),
        1,
        "exactly one generate must succeed: a={a:?}, b={b:?}"
    );

    // Post-condition: exactly one active row, regardless of who won.
    let active_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)
           FROM tenant_dek_wrappings
          WHERE tenant_id = $1
            AND record_kind = 'tenant_brand'
            AND state = 'active'",
    )
    .bind(tenant.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(active_count, 1);
}
