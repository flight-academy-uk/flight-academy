//! flight-academy-api — app builder (utoipa-axum OpenApiRouter, the
//! assembled OpenAPI document, the `app()`/`app_for_test()` factory pair).
//! Integration tests depend on this; the binary is a thin entrypoint per
//! ADR-005 §D.
//!
//! Single source of truth for the contract per ADR-006 §A: the same router
//! assembly that serves requests also produces the OpenAPI document the
//! `emit-spec` subcommand writes.
//!
//! Handler functions and their wire-shape response types live under
//! [`handlers`], one module per resource.

pub mod assets;
#[cfg(feature = "embedded-static")]
mod embedded;
mod error;
mod handlers;
mod middleware;

pub use error::{ApiError, ProblemDetails};
pub use handlers::audit_events::AuditEventCount;
pub use handlers::health::HealthResponse;
pub use handlers::tenants::{TenantDeleteRequest, TenantPatchRequest, TenantResponse};

use axum::http::{HeaderValue, header};
use tower::ServiceBuilder;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
#[cfg(not(feature = "embedded-static"))]
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Flight Academy API",
        description = "Multi-tenant aviation platform — UK CAA / EASA ATO, Part 145, airfield operator surfaces.",
    ),
    components(schemas(ProblemDetails))
)]
struct ApiDoc;

/// The assembled router and its OpenAPI document. Returned as a named pair
/// so callers can pick the half they need without touching tuple indices.
struct Built {
    router: axum::Router,
    openapi: utoipa::openapi::OpenApi,
}

fn build(with_dev_auth: bool) -> Built {
    // Tenant-scoped product-API routes. Production-mode wraps them in
    // dev_auth (the walking-skeleton subject extractor); test-mode omits
    // it so integration tests inject a `Subject` directly into request
    // extensions and exercise the policy + RLS path without env-var
    // dependence.
    let mut tenant_routes = OpenApiRouter::new()
        .routes(routes!(
            handlers::tenants::tenant_get,
            handlers::tenants::tenant_patch,
            handlers::tenants::tenant_delete
        ))
        .routes(routes!(handlers::audit_events::audit_event_count));
    if with_dev_auth {
        tenant_routes = tenant_routes.route_layer(axum::middleware::from_fn(middleware::dev_auth));
    }

    // MASH HTML surface (ADR-020). Plain axum routes — not in the OpenAPI
    // contract per §A — merged into the OpenApiRouter so they participate
    // in the same middleware stack (request-id propagation, security
    // headers).
    //
    // The `/_hx/<resource>/...` prefix marks server-rendered Maud
    // fragments that HTMX swaps into DOM slots on the originating page.
    // Separating them from the user-addressable URL space (`/`,
    // `/tenants/...`) makes the boundary visible — fragments are not
    // browser entry-points, never bookmarked, and CSP for HTMX swaps is
    // governed by the parent page's policy per ADR-020 §K.
    //
    // `/static/*` carries `Cache-Control: public, max-age=31536000,
    // immutable` per ADR-020 §I — URLs are content-hashed by `build.rs`
    // (e.g. `/static/app-a1b2c3d4.css`), so any stale cached entry is
    // always for the right bytes; touching the source shifts the URL.
    // The header layer applies only to the `/static/*` subtree; the
    // `if_not_present` semantics let the `security_headers` middleware
    // skip its `Cache-Control: no-store` default for this route.
    //
    // Static-asset source per ADR-020 §O depends on the build variant:
    //   • Default (hosted-production / dev): `ServeDir` reads from
    //     `apps/api/static/` on disk, relative to the binary's cwd
    //     (`cargo run` / `cargo test` from repo root resolve correctly).
    //   • `--features embedded-static`: every byte is baked into the
    //     binary at compile time via `rust-embed` (see `embedded.rs`),
    //     producing the self-host `flight-academy-embedded` artefact.
    // The wrapping `SetResponseHeaderLayer::if_not_present` applies
    // uniformly — keeping the cache policy in one place avoids two-paths
    // drift if the immutable URL strategy changes later.
    //
    // `Accept-Encoding` content negotiation per ADR-020 §I — server
    // preference order is zstd > brotli > gzip > identity. `build.rs`
    // emits `.zst`, `.br`, `.gz` siblings next to each hashed text
    // asset; `ServeDir` picks them up via `.precompressed_zstd()`,
    // `.precompressed_br()`, `.precompressed_gzip()` and the embedded
    // handler walks the same naming scheme manually. The order of the
    // builder methods matters: tower-http tries them in declaration
    // order, so zstd first → matches our priority.
    #[cfg(not(feature = "embedded-static"))]
    let static_inner = ServeDir::new("apps/api/static")
        .precompressed_zstd()
        .precompressed_br()
        .precompressed_gzip();
    #[cfg(feature = "embedded-static")]
    let static_inner = embedded::service();

    let immutable_static = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ))
        .service(static_inner);

    let html_routes = OpenApiRouter::new()
        .route("/", axum::routing::get(handlers::home::get))
        .route(
            "/_hx/home/server-id",
            axum::routing::get(handlers::home::server_id),
        )
        .nest_service("/static", immutable_static);

    // Layer ordering note (axum::Router::layer wraps each subsequent layer
    // OUTSIDE the previous one): `Propagate` is applied first so it ends up
    // INNER; `Set` is applied second so it ends up OUTER. On request, Set
    // runs first (insert/extract id into request extensions), then
    // Propagate; on response, Propagate runs first (copy id from extensions
    // onto the response header), then Set. That order is what makes the
    // outbound `x-request-id` header reliable per ADR-004 §B.
    //
    // `security_headers` is applied last so it ends up OUTERMOST — it sees
    // every response (handler output, 404s, 405s, errors from inner layers)
    // and is the load-bearing baseline per ADR-004 §F / ADR-015 §A.
    let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(handlers::health::healthz))
        .merge(tenant_routes)
        .merge(html_routes)
        .layer(PropagateRequestIdLayer::new(middleware::X_REQUEST_ID))
        .layer(SetRequestIdLayer::new(
            middleware::X_REQUEST_ID,
            middleware::MakeRequestUuidV7,
        ))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .split_for_parts();
    Built { router, openapi }
}

/// Construct the axum router used by the `serve` subcommand. Caller
/// attaches `Extension(Db)` after construction — the DB is a runtime
/// concern (serve has one, emit-spec doesn't).
pub fn app() -> axum::Router {
    build(true).router
}

/// Test-mode router. Identical to [`app`] except the `dev_auth` middleware
/// is omitted; tenant-scoped routes still require a `Subject` to be
/// present in request extensions but the test attaches it directly per
/// request rather than reading env vars.
pub fn app_for_test() -> axum::Router {
    build(false).router
}

/// Produce the assembled OpenAPI document used by the `emit-spec` subcommand
/// (and any tooling that wants to compare the live contract against the
/// committed `docs/api/openapi.json` — ADR-006 §A; format per ADR-018).
pub fn openapi() -> utoipa::openapi::OpenApi {
    build(true).openapi
}
