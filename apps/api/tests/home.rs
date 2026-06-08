//! Verifies the MASH HTML landing route (`/`) renders Maud markup, links the
//! Tailwind-compiled stylesheet, and participates in the same middleware stack
//! as the JSON API (ADR-020 §A / §K).
//!
//! No DB needed — the landing handler is stateless.

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn home_returns_html_200() {
    let app = flight_academy_api::app_for_test();
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    // Maud's `IntoResponse` wraps `Markup` with `text/html; charset=utf-8`.
    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("response has Content-Type")
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/html"),
        "expected HTML, got {content_type}"
    );

    // The security_headers layer still runs on this HTML response — same
    // baseline as the JSON API per ADR-020 §K.
    assert_eq!(
        resp.headers().get("x-frame-options").unwrap(),
        "DENY",
        "MASH HTML routes must carry the ADR-004 §F floor like the JSON surface",
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&bytes).unwrap();

    // Shape assertions — enough to catch a template that returns the wrong
    // page without over-coupling to copy that may rewrite freely.
    assert!(
        body.starts_with("<!DOCTYPE"),
        "must be a full HTML document"
    );
    assert!(body.contains("<title>Flight Academy</title>"));
    assert!(
        body.contains(r#"<link rel="stylesheet" href="/static/app.css">"#),
        "Tailwind-compiled stylesheet must be linked per ADR-020 §E",
    );
}
