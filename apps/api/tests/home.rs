//! Verifies the MASH HTML landing route (`/`) renders Maud markup,
//! links the Tailwind-compiled stylesheet and the vendored HTMX bundle,
//! and that the HTMX fragment endpoint (`/_hx/home/server-id`) returns
//! a bare Maud fragment with a freshly generated UUID v7 per ADR-020 §A
//! / §K / §F.
//!
//! No DB needed — the home resource is stateless.

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

    // Per-surface CSP from the handler must win over the middleware's
    // `default-src 'none'` JSON deny-all (the middleware uses `or_insert`,
    // so handler-set CSPs are preserved). The landing-page CSP must
    // permit the same-origin stylesheet, the vendored MASH scripts, AND
    // HTMX's XHR to the fragment endpoint; everything else stays denied.
    let csp = resp
        .headers()
        .get("content-security-policy")
        .expect("home handler must emit CSP")
        .to_str()
        .unwrap();
    assert!(
        csp.contains("default-src 'none'"),
        "CSP must default-deny — got: {csp}",
    );
    assert!(
        csp.contains("style-src 'self'"),
        "CSP must allow same-origin stylesheets so /static/app.css loads — got: {csp}",
    );
    assert!(
        csp.contains("script-src 'self'"),
        "CSP must allow same-origin scripts so vendored HTMX + Alpine load — got: {csp}",
    );
    assert!(
        csp.contains("connect-src 'self'"),
        "CSP must allow same-origin XHR so HTMX can fetch the fragment endpoint — got: {csp}",
    );
    assert!(
        !csp.contains("'unsafe-inline'"),
        "no 'unsafe-inline' — vendored bundles cover all script needs; got: {csp}",
    );
    assert!(
        !csp.contains("'unsafe-eval'"),
        "no 'unsafe-eval' — the vendored Alpine bundle is the CSP-safe build (@alpinejs/csp), which drops new Function() and works with strict CSP; got: {csp}",
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&bytes).unwrap();

    // Shape assertions — enough to catch a template that returns the
    // wrong page without over-coupling to copy that may rewrite freely.
    assert!(
        body.starts_with("<!DOCTYPE"),
        "must be a full HTML document"
    );
    assert!(body.contains("<title>Flight Academy</title>"));
    assert!(
        body.contains(r#"<link rel="stylesheet" href="/static/app.css">"#),
        "Tailwind-compiled stylesheet must be linked per ADR-020 §E",
    );
    assert!(
        body.contains(r#"src="/static/vendor/htmx.min.js""#),
        "vendored HTMX must be linked per ADR-020 §F",
    );
    assert!(
        body.contains(r#"hx-get="/_hx/home/server-id""#),
        "demo button must wire to the fragment endpoint",
    );
}

#[tokio::test]
async fn server_id_fragment_returns_bare_html_with_uuid_v7() {
    let app = flight_academy_api::app_for_test();
    let req = Request::builder()
        .uri("/_hx/home/server-id")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("fragment must have Content-Type")
        .to_str()
        .unwrap();
    assert!(
        content_type.starts_with("text/html"),
        "fragment must be text/html so HTMX swaps it; got {content_type}",
    );

    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = std::str::from_utf8(&bytes).unwrap();

    // HTMX inner-HTML swap requires a bare fragment — a leading
    // `<!DOCTYPE` or `<html>` would corrupt the document tree.
    assert!(
        !body.contains("<!DOCTYPE"),
        "fragment must not be a full document; got {body}",
    );
    assert!(
        !body.contains("<html"),
        "fragment must not contain <html>; got {body}",
    );
    assert!(
        body.contains("<span"),
        "fragment must wrap the id in a <span>; got {body}",
    );

    // The fragment carries a UUID v7 — the 13th hex char (after the 3rd
    // hyphen-delimited group) is the version nibble. Extract the id and
    // verify shape rather than coupling to a hard-coded value.
    let span_open = body.find('>').expect("opening <span> tag");
    let span_close = body.rfind("</span>").expect("closing </span>");
    let id = &body[span_open + 1..span_close];

    assert_eq!(id.len(), 36, "UUID is 36 chars including hyphens; got {id}");
    let bytes_ = id.as_bytes();
    assert_eq!(
        bytes_[14], b'7',
        "UUID v7 carries `7` at position 14; got {id}"
    );
}

#[tokio::test]
async fn server_id_fragment_changes_between_requests() {
    // Two calls to the same endpoint must produce different UUIDs —
    // proves the handler is genuinely fresh per request, not cached.
    async fn fetch_id(app: axum::Router) -> String {
        let req = Request::builder()
            .uri("/_hx/home/server-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = std::str::from_utf8(&bytes).unwrap().to_string();
        let span_open = body.find('>').expect("opening <span> tag");
        let span_close = body.rfind("</span>").expect("closing </span>");
        body[span_open + 1..span_close].to_string()
    }

    let first = fetch_id(flight_academy_api::app_for_test()).await;
    let second = fetch_id(flight_academy_api::app_for_test()).await;
    assert_ne!(
        first, second,
        "back-to-back fragments must produce different ids",
    );
}
