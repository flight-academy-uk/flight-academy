//! `/healthz` — liveness probe. No auth, no DB, no subject.

use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

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
pub async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
