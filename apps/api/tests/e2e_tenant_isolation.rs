//! End-to-end: HTTP request → ABAC policy → RLS-gated SELECT. Built via
//! `app_for_test()` so the dev_auth middleware is omitted and we attach
//! the `Subject` to each request directly (no env-var coupling, no
//! per-test process-global state). Uses `tower::ServiceExt::oneshot` —
//! no real listener, full middleware stack still runs.
//!
//! Paths use slugs per ADR-006 §C; the handler resolves slug → tenant_id
//! before policy evaluation. Unknown slug → 404 (not 403): slugs are
//! intended public identifiers.
#![allow(clippy::disallowed_methods)] // Test code: runtime SQL for per-test databases, seeding and catalogue checks.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode, header},
};
use flight_academy_api::{AppConfig, AuditKey};
use flight_academy_auth::ActorClass;
use flight_academy_core::TenantId;
use flight_academy_test_support::{
    fresh_db, member_subject, seed_tenant, seed_tenant_audit_events, tenant_admin_subject,
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn audit_count_tenant_match_serves_count() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    seed_tenant_audit_events(&db, alpha.id, 3).await;
    seed_tenant_audit_events(&db, bravo.id, 2).await;

    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/alpha-academy/audit-events/count")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["count"], 3);
}

#[tokio::test]
async fn audit_count_other_tenant_is_not_found() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    seed_tenant_audit_events(&db, alpha.id, 1).await;
    seed_tenant_audit_events(&db, bravo.id, 1).await;

    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // Subject is alpha; path is bravo. Bravo exists, but it is not visible
    // to alpha, so the slug does not resolve (ADR-033).
    let mut req = Request::builder()
        .uri("/api/v1/tenants/bravo-flight/audit-events/count")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn audit_count_unknown_slug_is_not_found() {
    let db = fresh_db().await;
    // No seeded tenants — every slug resolves to None.
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/no-such-tenant/audit-events/count")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(member_subject(TenantId::random()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_get_owner_sees_profile() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/alpha-academy")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["slug"], "alpha-academy");
    assert_eq!(body["name"], "Alpha Academy");
    assert_eq!(body["tenant_type"], "ato");
    assert_eq!(body["settings"], serde_json::json!({}));
    // The wire form is the prefixed public ID (ADR-006 §D), not the UUID.
    let id = body["id"].as_str().expect("id is a string");
    assert!(id.starts_with("ten_") && id.len() == 30, "{id}");
    assert_eq!(id.parse::<TenantId>().unwrap(), alpha.id);
    assert!(!id.contains(&alpha.id.as_uuid().to_string()));
}

#[tokio::test]
async fn tenant_get_other_tenant_is_not_found() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let _bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/bravo-flight")
        .body(Body::empty())
        .unwrap();
    // Subject belongs to alpha; asking for bravo, which exists.
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// The response for another tenant's slug must not differ from the one for
/// a slug nobody has, apart from the per-request ID, or it would tell a
/// caller which slugs exist (ADR-033).
#[tokio::test]
async fn other_tenant_and_unknown_slug_get_identical_responses() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let _bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut responses = Vec::new();
    for slug in ["bravo-flight", "no-such-tenant"] {
        let mut req = Request::builder()
            .uri(format!("/api/v1/tenants/{slug}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(member_subject(alpha.id));
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let mut headers = resp.headers().clone();
        headers.remove("x-request-id");
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let mut body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        body.as_object_mut().unwrap().remove("request_id");
        responses.push((status, headers, body));
    }
    assert_eq!(responses[0].0, StatusCode::NOT_FOUND);
    assert_eq!(responses[0], responses[1]);
}

/// Staff do not use the product API (ADR-010 §B). Without a tenant context
/// nothing is visible to them; with one, the tenant is visible and the
/// policy refuses the action.
#[tokio::test]
async fn staff_cannot_use_the_product_api() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    for (tenant_id, expected) in [
        (None, StatusCode::NOT_FOUND),
        (Some(alpha.id), StatusCode::FORBIDDEN),
    ] {
        let mut staff = member_subject(alpha.id);
        staff.actor_class = ActorClass::Staff;
        staff.tenant_id = tenant_id;
        let mut req = Request::builder()
            .uri("/api/v1/tenants/alpha-academy")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(staff);
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), expected, "staff with tenant {tenant_id:?}");
    }
}

#[tokio::test]
async fn tenant_get_unknown_slug_is_not_found() {
    let db = fresh_db().await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/nope")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut()
        .insert(member_subject(TenantId::random()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_patch_admin_updates_name() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db.clone()));

    let subject = tenant_admin_subject(alpha.id);
    let user_id = subject.user_id;

    let body = serde_json::json!({ "name": "Alpha Renamed" });
    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(subject);

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["name"], "Alpha Renamed");
    assert_eq!(v["slug"], "alpha-academy");

    // Audit event recorded — exactly one row for this tenant chain.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events
          WHERE chain_kind = 'tenant' AND chain_id = $1
            AND actor_id = $2",
    )
    .bind(alpha.id)
    .bind(user_id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "exactly one audit event for the rename");
}

#[tokio::test]
async fn tenant_patch_noop_does_not_audit() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db.clone()));

    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE chain_kind = 'tenant' AND chain_id = $1",
    )
    .bind(alpha.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(count, 0, "no-op PATCH should not audit");
}

#[tokio::test]
async fn tenant_patch_non_admin_is_forbidden() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"x"}"#))
        .unwrap();
    // Plain member, not tenant-admin.
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tenant_patch_cross_tenant_is_not_found() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let _bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // Subject is tenant-admin of alpha but patching bravo, which it cannot
    // see: the role does not reach another tenant.
    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/bravo-flight")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"hijack"}"#))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_patch_invalid_name_is_bad_request() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // Empty name fails the 1..=200 length check at the handler boundary.
    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":""}"#))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tenant_delete_admin_soft_deletes_and_audits() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db.clone()));

    let mut req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"deletion_reason":"requested_by_tenant"}"#))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Row is soft-deleted (deleted_at IS NOT NULL), not removed. The
    // WHERE clause also asserts the soft-delete happened — if it didn't
    // fetch_one would return RowNotFound.
    let deletion_reason: Option<String> = sqlx::query_scalar(
        "SELECT deletion_reason FROM tenants
          WHERE id = $1 AND deleted_at IS NOT NULL",
    )
    .bind(alpha.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(deletion_reason.as_deref(), Some("requested_by_tenant"));

    // Slug is no longer resolvable.
    assert!(
        db.visible_tenant_by_slug("alpha-academy", Some(alpha.id))
            .await
            .unwrap()
            .is_none()
    );

    // Audit recorded.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE chain_kind = 'tenant' AND chain_id = $1",
    )
    .bind(alpha.id)
    .fetch_one(db.pool())
    .await
    .unwrap();
    assert_eq!(count, 1, "exactly one audit event for the deletion");
}

#[tokio::test]
async fn tenant_delete_missing_reason_is_bad_request() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"deletion_reason":"   "}"#))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tenant_delete_oversized_reason_is_bad_request() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // 1001-char string exceeds the 1000-char cap.
    let oversized = "x".repeat(1001);
    let body = serde_json::json!({ "deletion_reason": oversized });
    let mut req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tenant_patch_on_soft_deleted_tenant_is_not_found() {
    // Contract test: PATCH on a soft-deleted tenant returns 404, not 500.
    // The slug-lookup path covers the obvious case (`resolve_tenant`
    // filters `deleted_at IS NULL`). The narrower race where the delete
    // wins *between* slug-lookup and the UPDATE is protected by
    // `try_tenant_patch_once`'s `fetch_optional` + `Option<None>` arm —
    // not directly exercised here (would need coordinated two-task
    // sequencing), but the end-state contract is the same.
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db.clone()));

    sqlx::query("UPDATE tenants SET deleted_at = now(), deletion_reason = 'test' WHERE id = $1")
        .bind(alpha.id)
        .execute(db.pool())
        .await
        .unwrap();

    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"x"}"#))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_delete_non_admin_is_forbidden() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"deletion_reason":"requested_by_tenant"}"#))
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn healthz_needs_no_subject() {
    let db = fresh_db().await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let req = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

/// Payloads on a tenant's chain, oldest first.
async fn chain_payloads(db: &flight_academy_db::Db, tenant: TenantId) -> Vec<serde_json::Value> {
    sqlx::query_scalar(
        "SELECT payload FROM audit_events
          WHERE chain_kind = 'tenant' AND chain_id = $1
          ORDER BY occurred_at, id",
    )
    .bind(tenant)
    .fetch_all(db.pool())
    .await
    .unwrap()
}

/// A router whose audit key is known, so tests can compute the hashes it
/// records.
fn app_with_known_key(db: &flight_academy_db::Db) -> (axum::Router, AuditKey) {
    let key = AuditKey::from_bytes(vec![7; 32]).unwrap();
    let config = AppConfig {
        audit_key: key.clone(),
        ..AppConfig::default()
    };
    let app = flight_academy_api::app_for_test_with(&config).layer(Extension(db.clone()));
    (app, key)
}

/// A not-found lookup is a Deny recorded on the caller's chain (ADR-033
/// §C). Another tenant's slug and an unknown slug write the same row apart
/// from the keyed hash of the slug as sent, and nothing is written to the
/// target's chain.
#[tokio::test]
async fn not_found_lookups_are_recorded_alike_on_the_callers_chain() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    let (app, key) = app_with_known_key(&db);

    for slug in ["bravo-flight", "no-such-tenant"] {
        let mut req = Request::builder()
            .uri(format!("/api/v1/tenants/{slug}"))
            .body(Body::empty())
            .unwrap();
        req.extensions_mut().insert(member_subject(alpha.id));
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    let rows = chain_payloads(&db, alpha.id).await;
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (row, slug) in rows.iter().zip(["bravo-flight", "no-such-tenant"]) {
        assert_eq!(
            *row,
            serde_json::json!({
                "action": "deny",
                "operation": "tenant.read",
                "route": "GET /api/v1/tenants/{tenant}",
                "requested": { "tenant_slug_hmac": key.mac_hex("tenant-slug", slug) },
                "reason": "not_visible_or_absent",
            })
        );
        assert!(!row.to_string().contains(slug), "slug text in {row}");
    }
    assert!(chain_payloads(&db, bravo.id).await.is_empty());
}

/// A policy Deny on a visible tenant is recorded with the policy's reason.
#[tokio::test]
async fn policy_denies_are_recorded_with_their_reason() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let (app, key) = app_with_known_key(&db);

    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"Renamed"}"#))
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let rows = chain_payloads(&db, alpha.id).await;
    assert_eq!(
        rows,
        vec![serde_json::json!({
            "action": "deny",
            "operation": "tenant.update",
            "route": "PATCH /api/v1/tenants/{tenant}",
            "requested": { "tenant_slug_hmac": key.mac_hex("tenant-slug", "alpha-academy") },
            "reason": "subject is not a tenant-admin",
        })]
    );
}

/// A path segment that is not a well-formed slug is recorded only as
/// malformed, so free text a caller types does not reach the audit chain.
#[tokio::test]
async fn malformed_keys_are_not_copied_into_the_audit_chain() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db.clone()));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/jane.doe@example.com")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let rows = chain_payloads(&db, alpha.id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["requested"],
        serde_json::json!({ "malformed": true })
    );
    assert!(!rows[0].to_string().contains("jane"));
}
