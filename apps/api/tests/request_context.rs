//! Request IDs, error-body request IDs and request size limits.

use axum::{
    Extension,
    body::Body,
    http::{Request, StatusCode, header},
};
use flight_academy_test_support::{fresh_db, member_subject, seed_tenant, tenant_admin_subject};
use http_body_util::BodyExt;
use tower::ServiceExt;
use uuid::Uuid;

fn request_id(resp: &axum::response::Response) -> String {
    resp.headers()
        .get("x-request-id")
        .expect("x-request-id on every response")
        .to_str()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn client_request_id_is_replaced_by_a_server_uuid_v7() {
    let app = flight_academy_api::app_for_test();
    let req = Request::builder()
        .uri("/healthz")
        .header("x-request-id", "client-chosen-id")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let id = request_id(&resp);
    assert_ne!(id, "client-chosen-id");
    let parsed = Uuid::parse_str(&id).expect("server request ID is a UUID");
    assert_eq!(parsed.get_version_num(), 7);
}

#[tokio::test]
async fn request_without_an_id_gets_one() {
    let app = flight_academy_api::app_for_test();
    let req = Request::builder()
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert!(Uuid::parse_str(&request_id(&resp)).is_ok());
}

#[tokio::test]
async fn problem_details_carry_the_response_request_id() {
    let db = fresh_db().await;
    let app = flight_academy_api::app_for_test().layer(Extension(db));
    let mut req = Request::builder()
        .uri("/api/v1/tenants/nope")
        .header("x-request-id", "client-chosen-id")
        .body(Body::empty())
        .unwrap();
    req.extensions_mut().insert(member_subject(Uuid::new_v4()));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let header_id = request_id(&resp);

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["request_id"], header_id.as_str());
}

#[tokio::test]
async fn oversized_body_is_rejected_with_413() {
    let db = fresh_db().await;
    let alpha = seed_tenant(&db, "alpha-academy", "Alpha Academy", "ato").await;
    let config = flight_academy_api::AppConfig {
        body_limit: 1024,
        ..flight_academy_api::AppConfig::default()
    };
    let app = flight_academy_api::app_for_test_with(&config).layer(Extension(db));

    let big = serde_json::json!({ "settings": { "note": "x".repeat(4096) } }).to_string();
    let mut req = Request::builder()
        .method("PATCH")
        .uri("/api/v1/tenants/alpha-academy")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(big))
        .unwrap();
    req.extensions_mut().insert(tenant_admin_subject(alpha.id));

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
