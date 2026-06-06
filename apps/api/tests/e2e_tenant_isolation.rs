//! End-to-end: HTTP request → ABAC policy → RLS-gated SELECT. Built via
//! `app_for_test()` so the dev_auth middleware is omitted and we attach
//! the `Subject` to each request directly (no env-var coupling, no
//! per-test process-global state). Uses `tower::ServiceExt::oneshot` —
//! no real listener, full middleware stack still runs.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode},
};
use flight_academy_test_support::{fresh_db, member_subject, seed_tenant_audit_events};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn http_isolation_tenant_match_serves_count() {
    let db = fresh_db().await;
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    seed_tenant_audit_events(&db, tenant_a, 3).await;
    seed_tenant_audit_events(&db, tenant_b, 2).await;

    let app = flight_academy_api::app_for_test().layer(Extension(db));

    let mut req = Request::builder()
        .uri(format!("/api/v1/tenants/{tenant_a}/audit-events/count"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(tenant_a));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["count"], 3);
}

#[tokio::test]
async fn http_isolation_tenant_mismatch_is_forbidden() {
    let db = fresh_db().await;
    let tenant_a = Uuid::new_v4();
    let tenant_b = Uuid::new_v4();
    seed_tenant_audit_events(&db, tenant_a, 1).await;
    seed_tenant_audit_events(&db, tenant_b, 1).await;

    let app = flight_academy_api::app_for_test().layer(Extension(db));

    // Subject is tenant_a; path is tenant_b. TenantOwnership denies
    // before any DB call.
    let mut req = Request::builder()
        .uri(format!("/api/v1/tenants/{tenant_b}/audit-events/count"))
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(tenant_a));

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
