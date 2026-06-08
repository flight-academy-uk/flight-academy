//! HTTP middleware:
//!
//! * `x-request-id` propagation per ADR-004 §B.
//! * `security_headers` — baseline response headers per ADR-004 §F, with
//!   CSP shaped for the current JSON-only surface (ADR-015's hash- and
//!   nonce-based machinery waits for the MASH HTML surface served by
//!   `apps/api` per ADR-020).
//! * `dev_auth` — walking-skeleton subject extractor; replaced by real
//!   passwordless auth (ADR-001 §F, ADR-013) when that lands.
//!
//! Each request gets a UUID v7 (time-ordered, sortable) if no `x-request-id`
//! header is supplied; the same id is echoed back on the response. Handlers
//! reach the id via the `tower_http::request_id::RequestId` request extension
//! (e.g. for populating the problem+json `request_id` field — see
//! `error::ProblemDetails`).
//!
//! Time-ordering note: UUID v7 leaks coarse timestamp, which is fine for an
//! ingress-generated correlation id. Clients supplying their own id may
//! choose any string-shaped value; we do not validate the format because
//! tower-http already constrains it to a valid `HeaderValue`.

use std::collections::BTreeSet;

use axum::{
    extract::Request,
    http::{HeaderValue, header::HeaderName},
    middleware::Next,
    response::Response,
};
use flight_academy_auth::{ActorClass, Role, Subject, SubjectAttributes};
use flight_academy_core::Error;
use tower_http::request_id::{MakeRequestId, RequestId};
use uuid::Uuid;

use crate::error::ApiError;

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Baseline response headers — ADR-004 §F's five-header floor plus four
/// modern additions from the OWASP Secure Headers Project recommendation.
/// ADR-004 §F frames its set as a "regression-tested floor, not a one-time
/// setup", so additions above the floor are in-scope without a refining
/// ADR.
///
/// **ADR-004 §F floor:**
///
/// * `Content-Security-Policy` — `default-src 'none'; frame-ancestors 'none';
///   base-uri 'none'; form-action 'none'`. ADR-015 §A's HTML-targeted shape
///   (`default-src 'self'` etc.) only makes sense once the MASH HTML surface
///   (Maud handlers in `apps/api`, ADR-020) ships routes with inline
///   scripts/styles; until then the API surface is JSON and "deny everything"
///   is correct. Each MASH route handler will emit its own per-surface CSP
///   (hash-based for prerendered + most SSR, nonce-based for sensitive routes
///   — ADR-015 §B/§C, refined by ADR-020 §K); this layer uses
///   `entry().or_insert()` so that handler-set headers are not clobbered.
/// * `Strict-Transport-Security` with `preload` — the `.app` TLD is HSTS-
///   preloaded at the registry level (ADR-004 §F), so `preload` reflects
///   reality. RFC 6797 says HSTS must not be emitted over plain HTTP, but
///   compliant browsers ignore it there anyway; we emit unconditionally
///   and rely on the TLS-terminating proxy / browser to do the right thing.
/// * `X-Frame-Options: DENY` — legacy companion to `frame-ancestors 'none'`
///   for older browsers; both directives say the same thing.
/// * `X-Content-Type-Options: nosniff` — refuse MIME-sniffing on JSON
///   responses, closing a classic content-confusion XSS path.
/// * `Referrer-Policy: strict-origin-when-cross-origin` — minimum
///   compatible with cross-origin links; same-origin requests still carry
///   the full referrer for diagnostics.
///
/// **OWASP additions (above the §F floor):**
///
/// * `Permissions-Policy` — denies a broad set of high-risk browser
///   features (sensors, camera, microphone, geolocation, payment, USB).
///   Browsers do not apply this to JSON responses directly, but it
///   shrinks the attack surface if our JSON is ever embedded in an HTML
///   context.
/// * `Cross-Origin-Resource-Policy: same-origin` — defence against
///   Spectre-class cross-origin reads via `<img>`/`<script>` embedding.
///   Same-origin HTMX requests from the MASH HTML surface (ADR-020) and
///   third-party integrations doing CORS are unaffected; CORP is the
///   additional layer that blocks embedding-as-resource.
/// * `Cross-Origin-Opener-Policy: same-origin` — browsing-context
///   isolation. Less directly relevant for non-HTML responses but
///   harmless.
/// * `Cache-Control: no-store` (default) — prevents intermediate /
///   browser caching of JSON responses that may carry tenant data.
///   Handlers that explicitly want caching (future `/openapi.json`,
///   static-asset routes) override via `entry().or_insert()` semantics.
///
/// **Deliberately omitted:** `X-XSS-Protection` (deprecated; past
/// misconfigurations were exploitable), `Public-Key-Pins` (deprecated),
/// `Expect-CT` (deprecated; CT enforcement is implicit), `Server`/
/// `X-Powered-By` removal (axum emits neither),
/// `Cross-Origin-Embedder-Policy` (only meaningful for HTML contexts
/// needing SharedArrayBuffer).
///
/// Wired outermost in [`crate::build`] so the headers apply to every
/// response — successful handler output, 404s from the router, 405s,
/// and error responses from inner middleware.
pub async fn security_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    use axum::http::header;

    // `or_insert` not `insert`: leaves handler-set values untouched. This
    // is the seam ADR-015 §B/§C (refined by ADR-020 §K) will use when MASH
    // HTML handlers in `apps/api` ship routes that need their own per-surface
    // CSP, and that future static-asset handlers will use for long-TTL
    // Cache-Control.
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(
            "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ));
    headers
        .entry(header::STRICT_TRANSPORT_SECURITY)
        .or_insert(HeaderValue::from_static(
            "max-age=63072000; includeSubDomains; preload",
        ));
    headers
        .entry(header::X_FRAME_OPTIONS)
        .or_insert(HeaderValue::from_static("DENY"));
    headers
        .entry(header::X_CONTENT_TYPE_OPTIONS)
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("strict-origin-when-cross-origin"));
    headers
        .entry(HeaderName::from_static("permissions-policy"))
        .or_insert(HeaderValue::from_static(
            "accelerometer=(), camera=(), geolocation=(), gyroscope=(), \
             magnetometer=(), microphone=(), payment=(), usb=()",
        ));
    headers
        .entry(HeaderName::from_static("cross-origin-resource-policy"))
        .or_insert(HeaderValue::from_static("same-origin"));
    headers
        .entry(HeaderName::from_static("cross-origin-opener-policy"))
        .or_insert(HeaderValue::from_static("same-origin"));
    headers
        .entry(header::CACHE_CONTROL)
        .or_insert(HeaderValue::from_static("no-store"));

    response
}

/// Generates a UUID v7 per request. tower-http's bundled `MakeRequestUuid`
/// uses v4; we want v7 for time-ordering in audit / log correlation.
#[derive(Clone, Copy, Default)]
pub struct MakeRequestUuidV7;

impl MakeRequestId for MakeRequestUuidV7 {
    fn make_request_id<B>(&mut self, _request: &axum::http::Request<B>) -> Option<RequestId> {
        // UUID v7 hyphenated form is always a valid HeaderValue (ASCII, no
        // control chars), so the conversion cannot fail.
        let id = Uuid::now_v7().to_string();
        HeaderValue::from_str(&id).ok().map(RequestId::new)
    }
}

/// Walking-skeleton identity middleware. Reads a `Subject` from the
/// `FA_DEV_USER_ID` and `FA_DEV_TENANT_ID` environment variables and
/// attaches it to request extensions. Gated by `FA_DEV_AUTH=1` so it
/// never activates by accident; with the env unset, all routes
/// downstream of this layer return 401.
///
/// Removed when the real passwordless / WebAuthn auth subsystem
/// (ADR-001 §F, ADR-013) lands. Documented as **dev-only** in CONTRIBUTING
/// once that subsystem provides the production path.
pub async fn dev_auth(mut req: Request, next: Next) -> Result<Response, ApiError> {
    if std::env::var("FA_DEV_AUTH").as_deref() != Ok("1") {
        return Err(Error::Unauthorized.into());
    }
    let user_id = std::env::var("FA_DEV_USER_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
        .ok_or(ApiError::from(Error::Unauthorized))?;
    let tenant_id = std::env::var("FA_DEV_TENANT_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
        .ok_or(ApiError::from(Error::Unauthorized))?;

    // Roles are comma-separated in `FA_DEV_ROLES` (e.g. `tenant-admin`).
    // Unknown role strings are silently dropped so a config ahead of the
    // binary's known set degrades safely. Empty env var → no roles
    // (subject is a plain member).
    let roles: BTreeSet<Role> = std::env::var("FA_DEV_ROLES")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| Role::from_str_known(s.trim()))
        .collect();

    let subject = Subject {
        user_id,
        actor_class: ActorClass::Member,
        tenant_id: Some(tenant_id),
        roles,
        attributes: SubjectAttributes,
        elevation: None,
    };
    req.extensions_mut().insert(subject);
    Ok(next.run(req).await)
}
