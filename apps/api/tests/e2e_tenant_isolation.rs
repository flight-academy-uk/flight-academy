//! End-to-end: HTTP request → ABAC policy → RLS-gated SELECT. Built via
//! `app_for_test()` so the dev_auth middleware is omitted and we attach
//! the `Subject` to each request directly (no env-var coupling, no
//! per-test process-global state). Uses `tower::ServiceExt::oneshot` —
//! no real listener, full middleware stack still runs.
//!
//! Paths use slugs per ADR-006 §C; the handler resolves slug → tenant_id
//! before policy evaluation. Unknown slug → 404 (not 403): slugs are
//! intended public identifiers.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode},
};
use flight_academy_test_support::{
    fresh_db, member_subject, seed_tenant, seed_tenant_audit_events,
};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

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
async fn audit_count_tenant_mismatch_is_forbidden() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    seed_tenant_audit_events(&db, alpha.id, 1).await;
    seed_tenant_audit_events(&db, bravo.id, 1).await;

    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // Subject is alpha; path is bravo. TenantOwnership denies before any
    // tenant-scoped DB call (the slug resolves successfully — the policy
    // check is what fails).
    let mut req = Request::builder()
        .uri("/api/v1/tenants/bravo-flight/audit-events/count")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
    req.extensions_mut().insert(member_subject(Uuid::new_v4()));

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
    assert_eq!(body["id"], alpha.id.to_string());
}

#[tokio::test]
async fn tenant_get_non_member_is_forbidden() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let _bravo = seed_tenant(&db, "bravo-flight", "Bravo Flight", "part_145").await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/bravo-flight")
        .body(Body::empty())
        .unwrap();
    // Subject belongs to alpha; asking for bravo.
    req.extensions_mut().insert(member_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tenant_get_unknown_slug_is_not_found() {
    let db = fresh_db().await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri("/api/v1/tenants/nope")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(Uuid::new_v4()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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
