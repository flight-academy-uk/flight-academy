//! `/` — landing page handler (the controller half of the resource).
//!
//! Owns HTTP concerns: routing, the per-surface CSP, response shaping.
//! Defers rendering to [`view::landing`], which returns pure Maud
//! [`maud::Markup`] with no `axum` or HTTP types. Splitting controller
//! from view is the per-resource pattern established by ADR-020 §D —
//! `mod.rs` is the controller, `view.rs` is the template, and the view
//! layer is unit-testable without the router because it has no HTTP
//! coupling.
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
/// surface; that deny-all CSP would also block the same-origin stylesheet
/// this page links to. This handler emits a tighter HTML-surface CSP that
/// allows same-origin styles only — no scripts, no fonts, no images,
/// no frames — matching the page's no-JS posture per ADR-020 §L.
///
/// Hash-based CSP per ADR-015 §B / ADR-020 §K (covering inline content
/// hashes for HTMX-driven SSR routes) lands when those routes exist.
const HOME_CSP: &str = "default-src 'none'; \
    style-src 'self'; \
    frame-ancestors 'none'; \
    base-uri 'none'; \
    form-action 'none'";

/// Render the landing page.
pub async fn get() -> impl IntoResponse {
    (
        [(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(HOME_CSP),
        )],
        view::landing(),
    )
}
