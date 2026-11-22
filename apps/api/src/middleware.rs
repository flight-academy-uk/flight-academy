//! HTTP middleware:
//!
//! * request IDs: every request gets a server-generated UUID v7. A client's
//!   `x-request-id` is never used as the ID; a sanitised copy is kept as
//!   `client_request_id` for log correlation only.
//! * `request_scope`: makes the request ID available to error responses.
//! * `make_span`: one tracing span per request carrying the request ID,
//!   method and matched route template (never the raw path).
//! * `security_headers`: baseline response headers per ADR-004 §F.
//! * `dev_auth`: development-only subject injection, configured once at
//!   startup; replaced by real passwordless authentication.

use axum::{
    extract::{MatchedPath, Request, State},
    http::{HeaderValue, header::HeaderName},
    middleware::Next,
    response::Response,
};
use flight_academy_auth::Subject;
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

/// The `x-request-id` a client sent, if it was short and plain enough to log.
/// Kept only for correlating with the client's own logs; never used as the
/// request ID.
#[derive(Clone, Debug)]
pub struct ClientRequestId(pub String);

const CLIENT_REQUEST_ID_MAX: usize = 64;

/// Remove any client-supplied `x-request-id` before `SetRequestIdLayer` runs,
/// so the server always generates the ID.
pub async fn strip_client_request_id(mut req: Request, next: Next) -> Response {
    if let Some(value) = req.headers_mut().remove(&X_REQUEST_ID) {
        let sanitised = value
            .to_str()
            .ok()
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= CLIENT_REQUEST_ID_MAX
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
            })
            .unwrap_or("invalid")
            .to_owned();
        req.extensions_mut().insert(ClientRequestId(sanitised));
    }
    next.run(req).await
}

tokio::task_local! {
    static REQUEST_ID: String;
}

/// The current request's ID, when called inside `request_scope`.
pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(Clone::clone).ok()
}

fn request_id_of<B>(req: &axum::http::Request<B>) -> &str {
    req.extensions()
        .get::<RequestId>()
        .and_then(|id| id.header_value().to_str().ok())
        .unwrap_or("")
}

/// Run the rest of the stack with the request ID in task-local scope, so
/// `ApiError` can put it in `ProblemDetails.request_id`, then make sure any
/// other error response is problem+json too.
pub async fn request_scope(req: Request, next: Next) -> Response {
    let id = request_id_of(&req).to_owned();
    let response = REQUEST_ID.scope(id.clone(), next.run(req)).await;
    crate::error::ensure_problem_json(response, &id)
}

/// Span for `TraceLayer`: request ID, method and route template. The raw
/// URI is not recorded, because paths can carry identifiers.
pub fn make_span<B>(req: &axum::http::Request<B>) -> tracing::Span {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or("unmatched");
    let client_request_id = req
        .extensions()
        .get::<ClientRequestId>()
        .map(|c| c.0.as_str())
        .unwrap_or("");
    tracing::info_span!(
        "request",
        request_id = request_id_of(req),
        method = %req.method(),
        route,
        client_request_id,
    )
}

/// Development-only identity middleware. The subject is built once at startup
/// from `FA_DEV_*` settings (see the `serve` command); when development
/// authentication is off, every route behind this layer returns 401.
/// Removed when real passwordless authentication lands.
pub async fn dev_auth(
    State(subject): State<Option<Subject>>,
    mut req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let Some(subject) = subject else {
        return Err(Error::Unauthorized.into());
    };
    req.extensions_mut().insert(subject);
    Ok(next.run(req).await)
}
