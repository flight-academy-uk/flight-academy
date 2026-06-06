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

use axum::{Extension, Json, extract::Path};
use flight_academy_auth::{
    Action, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Subject, TenantOwnership,
};
use flight_academy_core::Error;
use flight_academy_db::{Db, Tenant};
use serde::Serialize;
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;

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

#[derive(Serialize, ToSchema)]
pub struct AuditEventCount {
    pub count: i64,
}

/// Public read shape for a tenant. Mirrors `flight_academy_db::Tenant`
/// minus internals; sensitivity-classed fields (DEK wrapping, deletion
/// metadata, etc.) are not in the wire shape per ADR-008 §B.
#[derive(Serialize, ToSchema)]
pub struct TenantResponse {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub tenant_type: String,
    pub settings: serde_json::Value,
}

impl From<Tenant> for TenantResponse {
    fn from(t: Tenant) -> Self {
        Self {
            id: t.id,
            slug: t.slug,
            name: t.name,
            tenant_type: t.tenant_type,
            settings: t.settings,
        }
    }
}

/// Resolve a path-supplied slug to its tenant row, or map to a 404.
/// Centralised so every slug-keyed handler returns the same NotFound
/// shape and doesn't accidentally diverge.
async fn resolve_tenant(db: &Db, slug: &str) -> Result<Tenant, ApiError> {
    db.tenant_by_slug(slug)
        .await?
        .ok_or_else(|| Error::NotFound { resource: "tenant" }.into())
}

/// GET a tenant by slug. Returns the public read shape if the subject is
/// permitted to see this tenant (today: TenantOwnership, i.e. the subject
/// belongs to it).
#[utoipa::path(
    get,
    path = "/api/v1/tenants/{tenant}",
    params(
        ("tenant" = String, Path, description = "Tenant slug per ADR-006 §C."),
    ),
    responses(
        (status = 200, description = "Tenant profile", body = TenantResponse),
        (status = 401, description = "No authenticated subject", body = ProblemDetails),
        (status = 403, description = "Subject is not a member of this tenant", body = ProblemDetails),
        (status = 404, description = "No live tenant with that slug", body = ProblemDetails),
        (status = 500, description = "Internal error", body = ProblemDetails),
    ),
)]
async fn tenant_get(
    Extension(db): Extension<Db>,
    Extension(subject): Extension<Subject>,
    Path(slug): Path<String>,
) -> Result<Json<TenantResponse>, ApiError> {
    let tenant = resolve_tenant(&db, &slug).await?;
    let resource = Resource {
        tenant_id: tenant.id,
        kind: ResourceKind::Tenant,
        owner: None,
        attributes: ResourceAttributes,
    };
    match TenantOwnership.permit(&subject, Action::ReadTenant, &resource) {
        Decision::Permit => {}
        Decision::Deny { .. } | Decision::NotApplicable => return Err(Error::Forbidden.into()),
    }
    Ok(Json(tenant.into()))
}

/// Tenant-scoped audit-event count — the walking-skeleton round-trip that
/// proves ABAC + RLS end to end, now slug-addressed per ADR-006 §C.
/// Subject is extracted by `middleware::dev_auth`; the slug → id lookup
/// happens before policy evaluation (unknown slug → 404, mismatched tenant
/// → 403); `Db::begin_tenant` opens a transaction with `SET LOCAL ROLE
/// app_api` + the `app.current_tenant` GUC so the SELECT below is filtered
/// by the RLS policy on `audit_events`.
///
/// Real audit-event browsing belongs in the staff plane (`apps/admin`,
/// ADR-010 §I); this counts rows for the WS#4 demonstration without
/// exposing audit content.
#[utoipa::path(
    get,
    path = "/api/v1/tenants/{tenant}/audit-events/count",
    params(
        ("tenant" = String, Path, description = "Tenant slug per ADR-006 §C."),
    ),
    responses(
        (status = 200, description = "Audit-event count for the tenant", body = AuditEventCount),
        (status = 401, description = "No authenticated subject", body = ProblemDetails),
        (status = 403, description = "Subject's tenant does not match path tenant", body = ProblemDetails),
        (status = 404, description = "No live tenant with that slug", body = ProblemDetails),
        (status = 500, description = "Internal error", body = ProblemDetails),
    ),
)]
async fn audit_event_count(
    Extension(db): Extension<Db>,
    Extension(subject): Extension<Subject>,
    Path(slug): Path<String>,
) -> Result<Json<AuditEventCount>, ApiError> {
    let tenant = resolve_tenant(&db, &slug).await?;
    let resource = Resource {
        tenant_id: tenant.id,
        kind: ResourceKind::AuditEvent,
        owner: None,
        attributes: ResourceAttributes,
    };
    match TenantOwnership.permit(&subject, Action::ListAuditEvents, &resource) {
        Decision::Permit => {}
        Decision::Deny { .. } | Decision::NotApplicable => return Err(Error::Forbidden.into()),
    }

    let mut tx = db.begin_tenant(tenant.id).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events")
        .fetch_one(tx.conn())
        .await?;
    tx.commit().await?;

    Ok(Json(AuditEventCount { count }))
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

fn build(with_dev_auth: bool) -> Built {
    // Tenant-scoped product-API routes. Production-mode wraps them in
    // dev_auth (the walking-skeleton subject extractor); test-mode omits
    // it so integration tests inject a `Subject` directly into request
    // extensions and exercise the policy + RLS path without env-var
    // dependence.
    let mut tenant_routes = OpenApiRouter::new()
        .routes(routes!(tenant_get))
        .routes(routes!(audit_event_count));
    if with_dev_auth {
        tenant_routes = tenant_routes.route_layer(axum::middleware::from_fn(middleware::dev_auth));
    }

    // Layer ordering note (axum::Router::layer wraps each subsequent layer
    // OUTSIDE the previous one): `Propagate` is applied first so it ends up
    // INNER; `Set` is applied second so it ends up OUTER. On request, Set
    // runs first (insert/extract id into request extensions), then
    // Propagate; on response, Propagate runs first (copy id from extensions
    // onto the response header), then Set. That order is what makes the
    // outbound `x-request-id` header reliable per ADR-004 §B.
    let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(healthz))
        .merge(tenant_routes)
        .layer(PropagateRequestIdLayer::new(middleware::X_REQUEST_ID))
        .layer(SetRequestIdLayer::new(
            middleware::X_REQUEST_ID,
            middleware::MakeRequestUuidV7,
        ))
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
