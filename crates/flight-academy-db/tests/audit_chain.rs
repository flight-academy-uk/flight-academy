//! Verifies the audit chain writer produces correctly-linked rows and
//! that a tampered row is detected by re-deriving its hash.

use flight_academy_db::Db;
use flight_academy_test_support::{fresh_db, seed_tenant};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

#[tokio::test]
async fn chain_links_across_three_writes() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "chain-test", "Chain Test", "ato").await;
    let actor = Uuid::new_v4();

    let a = db
        .write_tenant_audit_event(
            "member",
            Some(actor),
            tenant.id,
            serde_json::json!({"step": 1}),
        )
        .await
        .unwrap();
    let b = db
        .write_tenant_audit_event(
            "member",
            Some(actor),
            tenant.id,
            serde_json::json!({"step": 2}),
        )
        .await
        .unwrap();
    let c = db
        .write_tenant_audit_event(
            "member",
            Some(actor),
            tenant.id,
            serde_json::json!({"step": 3}),
        )
        .await
        .unwrap();

    // First row has no predecessor; subsequent rows link to the prior
    // payload_hash.
    assert!(a.prev_hash.is_none(), "first row has NULL prev_hash");
    assert_eq!(
        b.prev_hash.as_deref(),
        Some(a.payload_hash.as_slice()),
        "b.prev_hash == a.payload_hash"
    );
    assert_eq!(
        c.prev_hash.as_deref(),
        Some(b.payload_hash.as_slice()),
        "c.prev_hash == b.payload_hash"
    );
}

#[tokio::test]
async fn rederiving_payload_hash_matches_stored_value() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "derive-test", "Derive Test", "part_145").await;
    let actor = Uuid::new_v4();

    let payload = serde_json::json!({"action": "test", "n": 42});
    let written = db
        .write_tenant_audit_event("member", Some(actor), tenant.id, payload.clone())
        .await
        .unwrap();

    // Re-derive the payload_hash from the persisted constituent fields.
    // Mirrors the path the periodic verifier will take per ADR-004 §H.
    let derived = Db::audit_payload_hash(
        written.occurred_at,
        "member",
        Some(actor),
        Some(tenant.id),
        "tenant",
        Some(tenant.id),
        &payload,
        written.prev_hash.as_deref(),
    );
    assert_eq!(
        derived, written.payload_hash,
        "re-derived hash matches what was stored"
    );
}

#[tokio::test]
async fn tampered_payload_fails_rederivation() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "tamper-test", "Tamper Test", "airfield_operator").await;
    let actor = Uuid::new_v4();

    let original = serde_json::json!({"action": "create", "id": 1});
    let written = db
        .write_tenant_audit_event("member", Some(actor), tenant.id, original)
        .await
        .unwrap();

    // Simulate an attacker who flipped a byte in the payload after the
    // row was written. The persisted payload_hash and prev_hash stay the
    // same; only the payload changes.
    let tampered = serde_json::json!({"action": "create", "id": 2});
    let derived_with_tampered = Db::audit_payload_hash(
        written.occurred_at,
        "member",
        Some(actor),
        Some(tenant.id),
        "tenant",
        Some(tenant.id),
        &tampered,
        written.prev_hash.as_deref(),
    );
    assert_ne!(
        derived_with_tampered, written.payload_hash,
        "tampered payload re-derivation must not match the stored hash"
    );
}

#[tokio::test]
async fn isolated_chains_dont_cross_link() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-chain", "Alpha", "ato").await;
    let bravo = seed_tenant(&db, "bravo-chain", "Bravo", "ato").await;
    let actor = Uuid::new_v4();

    // Interleave writes: alpha, bravo, alpha, bravo.
    let a1 = db
        .write_tenant_audit_event("member", Some(actor), alpha.id, serde_json::json!({"x": 1}))
        .await
        .unwrap();
    let b1 = db
        .write_tenant_audit_event("member", Some(actor), bravo.id, serde_json::json!({"x": 2}))
        .await
        .unwrap();
    let a2 = db
        .write_tenant_audit_event("member", Some(actor), alpha.id, serde_json::json!({"x": 3}))
        .await
        .unwrap();
    let b2 = db
        .write_tenant_audit_event("member", Some(actor), bravo.id, serde_json::json!({"x": 4}))
        .await
        .unwrap();

    // Each chain stands alone — interleaved writes do not link across
    // chains. alpha's second row points at alpha's first; same for bravo.
    assert!(a1.prev_hash.is_none(), "alpha chain starts NULL");
    assert!(b1.prev_hash.is_none(), "bravo chain starts NULL");
    assert_eq!(a2.prev_hash.as_deref(), Some(a1.payload_hash.as_slice()));
    assert_eq!(b2.prev_hash.as_deref(), Some(b1.payload_hash.as_slice()));
}

#[tokio::test]
async fn writer_inserts_row_visible_via_select() {
    // Verifies the writer actually persists the row (and not just returns
    // the in-memory AuditEvent). Uses pool query so the SELECT goes
    // through the same session role as the writer — no RLS interference.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "select-test", "Select Test", "ato").await;
    let actor = Uuid::new_v4();

    let written = db
        .write_tenant_audit_event(
            "member",
            Some(actor),
            tenant.id,
            serde_json::json!({"verify": true}),
        )
        .await
        .unwrap();

    let row = sqlx::query("SELECT payload_hash FROM audit_events WHERE id = $1")
        .bind(written.id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let stored: Vec<u8> = row.try_get("payload_hash").unwrap();
    assert_eq!(stored, written.payload_hash);
}

#[allow(dead_code)]
fn _types_compile() {
    // Compile-time sanity that OffsetDateTime is reachable here.
    let _: Option<OffsetDateTime> = None;
}
