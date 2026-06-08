//! Verifies the MASH HTML landing route (`/`), the HTMX fragment
//! endpoint (`/_hx/home/server-id`), and the content-hashed
//! `/static/*` asset surface — per ADR-020 §A / §F / §I / §K.
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
        "CSP must allow same-origin stylesheets so the Tailwind bundle loads — got: {csp}",
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

    // Per-route Cache-Control per ADR-020 §I: marketing chrome is
    // edge-cacheable for an hour with a day of stale-while-revalidate.
    // The handler-set header must win over the middleware's no-store
    // default (or_insert semantics).
    let cache_control = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .expect("home handler must emit Cache-Control")
        .to_str()
        .unwrap();
    assert!(
        cache_control.contains("public"),
        "landing must be public-cacheable — got: {cache_control}",
    );
    assert!(
        cache_control.contains("s-maxage=3600"),
        "landing must be edge-cacheable for an hour per ADR-020 §I — got: {cache_control}",
    );
    assert!(
        cache_control.contains("stale-while-revalidate=86400"),
        "landing must allow a day of SWR per ADR-020 §I — got: {cache_control}",
    );
    assert!(
        !cache_control.contains("no-store"),
        "landing must override the middleware no-store default — got: {cache_control}",
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

    // Content-hashed CSS link — assert the shape rather than the exact
    // hash, which shifts whenever the Tailwind output changes. The
    // hashed URL guarantee (build.rs renames in place) is itself
    // covered by the unit test in `view::tests`.
    assert!(
        body.contains(r#"<link rel="stylesheet" href="/static/app-"#),
        "Tailwind-compiled stylesheet must be linked at a hashed URL per ADR-020 §E + §I",
    );
    assert!(
        body.contains(r#"<script src="/static/vendor/htmx-"#),
        "vendored HTMX must be linked at a hashed URL per ADR-020 §F + §I",
    );
    assert!(
        body.contains(r#"hx-get="/_hx/home/server-id""#),
        "demo button must wire to the fragment endpoint",
    );
}

#[tokio::test]
async fn static_assets_carry_immutable_cache_control() {
    // ServeDir is wrapped with a `SetResponseHeaderLayer::if_not_present`
    // that emits `public, max-age=31536000, immutable` on every
    // `/static/*` response. The URL itself is content-addressed via
    // the hash in the filename, so a stale CDN entry can never serve
    // the wrong bytes — `immutable` is correct (ADR-020 §I).
    //
    // We do not need a real file on disk to verify the layer is wired:
    // ServeDir will 404 on an unknown path and the header layer applies
    // to every response that flows through the static service.
    let app = flight_academy_api::app_for_test();
    let req = Request::builder()
        .uri("/static/does-not-exist.css")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    let cache_control = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .expect("/static/* must emit Cache-Control via the layer")
        .to_str()
        .unwrap();
    assert!(
        cache_control.contains("public"),
        "/static/* must be public-cacheable — got: {cache_control}",
    );
    assert!(
        cache_control.contains("max-age=31536000"),
        "/static/* must be cached for a year per ADR-020 §I — got: {cache_control}",
    );
    assert!(
        cache_control.contains("immutable"),
        "/static/* URLs are content-hashed → safe to mark immutable per ADR-020 §I — got: {cache_control}",
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

    // Per ADR-020 §I HTMX fragment endpoints carry `private, no-cache`
    // — `private` because fragments may hold per-user data, `no-cache`
    // so a browser MUST revalidate before reuse. Not `no-store`:
    // `no-cache` permits conditional-GET / `304 Not Modified` once
    // handlers gain `ETag` support, which `no-store` would preclude.
    // The handler overrides the middleware no-store default
    // explicitly (the security_headers middleware uses or_insert).
    let cache_control = resp
        .headers()
        .get(header::CACHE_CONTROL)
        .expect("fragment must carry Cache-Control")
        .to_str()
        .unwrap();
    assert!(
        cache_control.contains("private"),
        "fragment endpoints must be private per ADR-020 §I — got: {cache_control}",
    );
    assert!(
        cache_control.contains("no-cache"),
        "fragment endpoints must be no-cache per ADR-020 §I — got: {cache_control}",
    );
    assert!(
        !cache_control.contains("no-store"),
        "fragment endpoints must NOT be no-store — no-cache permits ETag/304 once added; got: {cache_control}",
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
