//! `/` — landing page handler (the controller half of the resource).
//!
//! Owns HTTP concerns: routing, the per-surface CSP, response shaping.
//! Defers rendering to [`view::landing`] and [`view::server_id_fragment`],
//! which return pure Maud [`maud::Markup`] with no `axum` or HTTP types.
//! Splitting controller from view is the per-resource pattern
//! established by ADR-020 §D — `mod.rs` is the controller, `view.rs` is
//! the template, and the view layer is unit-testable without the router
//! because it has no HTTP coupling.
//!
//! Not in the OpenAPI contract per ADR-020 §A — the HTML surface is
//! parallel to `/api/v1/*`, not a member of it. Registered as a plain
//! Axum route in `crate::build` after `OpenApiRouter::split_for_parts`.

mod view;

use axum::{
    http::{HeaderValue, header},
    response::IntoResponse,
};

/// Per-surface CSP for the landing page. The baseline `security_headers`
/// middleware emits `default-src 'none' …` via `or_insert` for the JSON
/// surface; that deny-all CSP would block the same-origin stylesheet,
/// the vendored MASH scripts, AND the HTMX XHR back to the fragment
/// endpoint. This handler emits a tighter HTML-surface CSP that allows
/// only what the landing page actually uses:
///
/// - `script-src 'self'` — vendored HTMX + Alpine (CSP-safe build,
///   `@alpinejs/csp`) bundles served from `/static/vendor/`. No
///   `'unsafe-inline'`, no `'unsafe-eval'`. The CSP-safe Alpine drops
///   `new Function()` and accepts registered component names in
///   `x-data` etc., not arbitrary JS expressions — strict CSP holds
///   without relaxation (ADR-020 §F / §K).
/// - `style-src 'self'` — Tailwind-compiled bundle at `/static/app.css`.
/// - `connect-src 'self'` — HTMX issues XHR/fetch back to the origin
///   for fragment endpoints (e.g. `/_hx/home/server-id`).
/// - `frame-ancestors 'none' / base-uri 'none' / form-action 'none'`
///   carry the ADR-004 §F floor for surfaces that have neither frames,
///   `<base>`, nor forms.
///
/// Tighter than ADR-015 §A's `default-src 'self'` baseline by design —
/// the landing page has no images, no fonts, no media, no `<iframe>`,
/// no `<form>`. Any future grant happens at the per-route layer when a
/// surface actually needs it.
///
/// Hash-based CSP per ADR-015 §B / ADR-020 §K (covering inline content
/// hashes for HTMX-driven SSR routes) is still future work; this CSP is
/// the conservative shape that works for an inline-content-free shell.
const HOME_CSP: &str = "default-src 'none'; \
    script-src 'self'; \
    style-src 'self'; \
    connect-src 'self'; \
    frame-ancestors 'none'; \
    base-uri 'none'; \
    form-action 'none'";

/// `Cache-Control` for the landing page per ADR-020 §I — public,
/// edge-cacheable for an hour, served stale for up to a day while a
/// fresh copy revalidates in the background. The page is prerendered
/// marketing chrome with no per-user state today; the asset URLs it
/// references are themselves content-hashed (`/static/app-<hash>.css`,
/// etc.), so a tenant-specific brand or copy change shifts the body
/// bytes and ETag without breaking the cache strategy. The
/// `security_headers` middleware emits `Cache-Control: no-store` via
/// `or_insert` for the JSON surface; this handler sets the header
/// first so the middleware's fallback is skipped.
const HOME_CACHE_CONTROL: &str = "public, s-maxage=3600, stale-while-revalidate=86400";

/// Render the landing page.
pub async fn get() -> impl IntoResponse {
    (
        [
            (
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(HOME_CSP),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static(HOME_CACHE_CONTROL),
            ),
        ],
        view::landing(),
    )
}

/// HTMX fragment endpoint — `GET /_hx/home/server-id`.
///
/// Returns a Maud fragment (no `<html>` chrome) carrying a freshly
/// generated UUID v7. HTMX swaps the fragment into `#server-id-slot` on
/// the landing page; the `text/html` content-type is supplied by Maud's
/// `IntoResponse` impl. Demonstrates the MASH client/server roundtrip:
/// browser issues XHR (allowed by `connect-src 'self'`), server renders
/// a partial Maud template, HTMX patches the DOM.
///
/// No CSP header set on the fragment response — it is swapped into the
/// landing page's DOM, so the landing page's CSP governs anything that
/// would execute. The fragment itself contains zero JS / inline styles /
/// other live content; it is a `<span>` with the id.
pub async fn server_id() -> impl IntoResponse {
    let id = uuid::Uuid::now_v7();
    view::server_id_fragment(&id.to_string())
}
