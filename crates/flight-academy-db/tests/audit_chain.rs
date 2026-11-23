//! Verifies the audit chain writer produces correctly-linked rows and
//! that a tampered row is detected by re-deriving its hash.
#![allow(clippy::disallowed_methods)] // Test code: runtime SQL for per-test databases, seeding and catalogue checks.

use flight_academy_core::UserId;
use flight_academy_test_support::{fresh_db, seed_tenant};
use sqlx::Row;

#[tokio::test]
async fn chain_links_across_three_writes() {
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "chain-test", "Chain Test", "ato").await;
    let actor = UserId::new();

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
    let actor = UserId::new();

    let payload = serde_json::json!({"action": "test", "n": 42});
    let written = db
        .write_tenant_audit_event("member", Some(actor), tenant.id, payload.clone())
        .await
        .unwrap();

    // Re-derive the payload_hash from the persisted constituent fields.
    // Mirrors the path the periodic verifier will take per ADR-004 §H.
    let derived = flight_academy_db::audit::payload_hash(
        written.occurred_at,
        "member",
        Some(actor.as_uuid()),
        Some(tenant.id.as_uuid()),
        "tenant",
        Some(tenant.id.as_uuid()),
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
    let actor = UserId::new();

    let original = serde_json::json!({"action": "create", "id": 1});
    let written = db
        .write_tenant_audit_event("member", Some(actor), tenant.id, original)
        .await
        .unwrap();

    // Simulate an attacker who flipped a byte in the payload after the
    // row was written. The persisted payload_hash and prev_hash stay the
    // same; only the payload changes.
    let tampered = serde_json::json!({"action": "create", "id": 2});
    let derived_with_tampered = flight_academy_db::audit::payload_hash(
        written.occurred_at,
        "member",
        Some(actor.as_uuid()),
        Some(tenant.id.as_uuid()),
        "tenant",
        Some(tenant.id.as_uuid()),
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
    let actor = UserId::new();

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
    let actor = UserId::new();

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

#[tokio::test]
async fn invalid_actor_class_is_rejected_by_check_constraint() {
    // Backstop for the runtime-only enforcement of `actor_class`. The
    // writer takes `&'static str` (see audit::write_tenant_audit_event)
    // and the DB CHECK is the load-bearing defence until ActorClass
    // moves to flight-academy-core; this test makes sure that defence
    // stays loaded.
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "check-test", "Check Test", "ato").await;

    let err = db
        .write_tenant_audit_event(
            "admin",
            Some(UserId::new()),
            tenant.id,
            serde_json::json!({}),
        )
        .await
        .expect_err("admin is not in the actor_class CHECK whitelist");

    // SQLSTATE 23514 is `check_violation`.
    let msg = format!("{err:?}");
    assert!(
        msg.contains("check") || msg.contains("23514") || msg.contains("constraint"),
        "expected a CHECK violation, got: {msg}"
    );
}

#[test]
fn rfc3339_formatter_byte_stable() {
    // Canary against silent `time` crate behaviour drift. The audit
    // chain's hash covers the RFC 3339 byte representation of
    // `occurred_at`; if `time` ever changes the format (trailing zeros,
    // fractional-second width, offset sigil, etc.) without us noticing,
    // every freshly-written chain would diverge from every chain written
    // before. Two assertions: one whole-second, one microsecond. The
    // fractional-second case is the one that actually exercises
    // production input shape — `SELECT now()` returns microsecond
    // precision and the more likely drift is in fractional-second
    // elision rules.
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    let dt_whole = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
    assert_eq!(dt_whole.format(&Rfc3339).unwrap(), "2023-11-14T22:13:20Z");

    let dt_frac = OffsetDateTime::from_unix_timestamp_nanos(1_700_000_000_123_456_000).unwrap();
    assert_eq!(
        dt_frac.format(&Rfc3339).unwrap(),
        "2023-11-14T22:13:20.123456Z"
    );
}

#[test]
fn serde_jcs_canonical_bytes_stable() {
    // Companion to `rfc3339_formatter_byte_stable` — pins one known good
    // JCS output for a representative payload shape. serde_jcs is the
    // load-bearing canonicalisation step; if it ever changes its output
    // (key ordering edge cases, Unicode normalisation, number formatting)
    // on a patch bump, every freshly-written chain would diverge from
    // every chain written before. This locks in the byte representation
    // for one fixed input as a regression backstop.
    let v = serde_json::json!({
        "action": "tenant.update",
        "fields": ["name", "settings"],
        "z_last": 1,
        "a_first": 2,
        "nested": {"b": true, "a": null}
    });
    let bytes = serde_jcs::to_vec(&v).expect("jcs encode");
    // JCS sorts keys lexicographically at every level. The expected
    // string is computed once and pinned; any change in the canonical
    // output ripples through every chain we've ever written.
    let expected = r#"{"a_first":2,"action":"tenant.update","fields":["name","settings"],"nested":{"a":null,"b":true},"z_last":1}"#;
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        expected,
        "serde_jcs canonical output drifted"
    );
}

/// Concurrent writers on one chain queue on the chain's head row, so every
/// write succeeds and the chain stays linear.
///
/// Linearity is checked by following the hash links rather than timestamp
/// order: exactly one row starts the chain, no row has two successors, and
/// every row is reachable from the first. The head must then point at the
/// last row and count every row.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_writes_on_one_chain_succeed_and_stay_linear() {
    const WRITERS: usize = 16;
    let db = fresh_db().await;
    let tenant = seed_tenant(&db, "chain-concurrent", "Chain Concurrent", "ato").await;

    let writes: Vec<_> = (0..WRITERS)
        .map(|i| {
            let db = db.clone();
            let tenant_id = tenant.id;
            tokio::spawn(async move {
                db.write_tenant_audit_event(
                    "member",
                    Some(UserId::new()),
                    tenant_id,
                    serde_json::json!({ "writer": i }),
                )
                .await
            })
        })
        .collect();
    for write in writes {
        write
            .await
            .expect("task joins")
            .expect("every concurrent write succeeds");
    }

    let rows: Vec<(Vec<u8>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT payload_hash, prev_hash FROM audit_events
          WHERE chain_kind = 'tenant' AND chain_id = $1",
    )
    .bind(tenant.id)
    .fetch_all(db.pool())
    .await
    .unwrap();
    assert_eq!(rows.len(), WRITERS, "one row per write");

    let starts: Vec<_> = rows.iter().filter(|(_, prev)| prev.is_none()).collect();
    assert_eq!(starts.len(), 1, "exactly one row starts the chain");
    let mut current = starts[0].0.clone();
    let mut reached = 1;
    loop {
        let successors: Vec<_> = rows
            .iter()
            .filter(|(_, prev)| prev.as_deref() == Some(current.as_slice()))
            .collect();
        assert!(
            successors.len() <= 1,
            "fork: {} rows link to the same predecessor",
            successors.len()
        );
        match successors.first() {
            Some((hash, _)) => {
                current = hash.clone();
                reached += 1;
            }
            None => break,
        }
    }
    assert_eq!(
        reached, WRITERS,
        "every row is reachable from the first by its links"
    );

    let (seq, tip_hash): (i64, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT seq, tip_hash FROM audit_chain_heads
          WHERE chain_kind = 'tenant' AND chain_id = $1",
    )
    .bind(tenant.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(seq, WRITERS as i64, "the head counts every row");
    assert_eq!(
        tip_hash.as_deref(),
        Some(current.as_slice()),
        "the head points at the last row"
    );
}
