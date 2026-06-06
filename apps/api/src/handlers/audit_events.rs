//! Audit-events handlers — tenant-scoped read surface (`count`) on the
//! product-API plane. Real audit browsing belongs in the staff plane
//! (`apps/admin`, ADR-010 §I).

use axum::{Extension, Json, extract::Path};
use flight_academy_auth::{
    Action, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Subject, TenantOwnership,
};
use flight_academy_core::Error;
use flight_academy_db::Db;
use serde::Serialize;
use utoipa::ToSchema;

use crate::error::{ApiError, ProblemDetails};
use crate::handlers::tenants::resolve_tenant;

#[derive(Serialize, ToSchema)]
pub struct AuditEventCount {
    pub count: i64,
}

/// Tenant-scoped audit-event count — the walking-skeleton round-trip that
/// proves ABAC + RLS end to end. The slug → id lookup happens before
/// policy evaluation (unknown slug → 404, mismatched tenant → 403);
/// `Db::begin_tenant` opens a transaction with `SET LOCAL ROLE app_api`
/// + the `app.current_tenant` GUC so the SELECT below is filtered by the
/// RLS policy on `audit_events`.
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
pub async fn audit_event_count(
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
