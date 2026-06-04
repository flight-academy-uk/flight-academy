//! flight-academy-api — app builder (utoipa-axum OpenApiRouter, handlers,
//! middleware, the assembled OpenAPI document). Integration tests depend on
//! this; the binary is a thin entrypoint per ADR-005 §D.
//!
//! Single source of truth for the contract per ADR-006 §A: the same router
//! assembly that serves requests also produces the OpenAPI document the
//! `emit-spec` subcommand writes.

use axum::Json;
use serde::Serialize;
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
#[openapi(info(
    title = "Flight Academy API",
    description = "Multi-tenant aviation platform — UK CAA / EASA ATO, Part 145, airfield operator surfaces.",
))]
struct ApiDoc;

fn build() -> (axum::Router, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(healthz))
        .split_for_parts()
}

/// Construct the axum router used by the `serve` subcommand and integration tests.
pub fn app() -> axum::Router {
    build().0
}

/// Produce the assembled OpenAPI document used by the `emit-spec` subcommand
/// (and any tooling that wants to compare the live contract against the
/// committed `docs/api/openapi.yaml`).
pub fn openapi() -> utoipa::openapi::OpenApi {
    build().1
}
