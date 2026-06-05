//! flight-academy-api — app builder (utoipa-axum OpenApiRouter, handlers,
//! middleware, the assembled OpenAPI document). Integration tests depend on
//! this; the binary is a thin entrypoint per ADR-005 §D.
//!
//! Single source of truth for the contract per ADR-006 §A: the same router
//! assembly that serves requests also produces the OpenAPI document the
//! `emit-spec` subcommand writes.

mod error;
mod middleware;

pub use error::{ApiError, ProblemDetails};

use axum::Json;
use serde::Serialize;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: &'static str,
}

/// Liveness probe. Returned 200 means the process is running and the HTTP
/// stack is responsive; readiness (DB, KMS, object store) will live behind
/// a separate `/readyz` per ADR-002 §G when those dependencies land.
#[utoipa::path(
    get,
    path = "/healthz",
    responses(
        (status = 200, description = "Service is up", body = HealthResponse),
    ),
)]
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

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

fn build() -> Built {
    // Layer ordering note (axum::Router::layer wraps each subsequent layer
    // OUTSIDE the previous one): `Propagate` is applied first so it ends up
    // INNER; `Set` is applied second so it ends up OUTER. On request, Set
    // runs first (insert/extract id into request extensions), then
    // Propagate; on response, Propagate runs first (copy id from extensions
    // onto the response header), then Set. That order is what makes the
    // outbound `x-request-id` header reliable per ADR-004 §B.
    let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(healthz))
        .layer(PropagateRequestIdLayer::new(middleware::X_REQUEST_ID))
        .layer(SetRequestIdLayer::new(
            middleware::X_REQUEST_ID,
            middleware::MakeRequestUuidV7,
        ))
        .split_for_parts();
    Built { router, openapi }
}

/// Construct the axum router used by the `serve` subcommand and integration tests.
pub fn app() -> axum::Router {
    build().router
}

/// Produce the assembled OpenAPI document used by the `emit-spec` subcommand
/// (and any tooling that wants to compare the live contract against the
/// committed `docs/api/openapi.json` — ADR-006 §A; format per ADR-018).
pub fn openapi() -> utoipa::openapi::OpenApi {
    build().openapi
}
