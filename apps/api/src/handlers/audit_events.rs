//! Audit-events handlers — tenant-scoped read surface (`count`) on the
//! product-API plane. Real audit browsing belongs in the staff plane
//! (`apps/admin`, ADR-010 §I).

use axum::{
    Extension, Json,
    extract::{MatchedPath, Path},
    http::Method,
};
use flight_academy_auth::{Action, Resource, ResourceKind, Subject, TenantOwnership};
use flight_academy_db::Db;
use serde::Serialize;
use utoipa::ToSchema;

use crate::deny::{AuditKey, DenyContext};
use crate::error::{ApiError, ProblemDetails};
use crate::handlers::tenants::resolve_tenant;

#[derive(Serialize, ToSchema)]
pub struct AuditEventCount {
    pub count: i64,
}

/// Tenant-scoped audit-event count — the walking-skeleton round-trip that
/// proves ABAC + RLS end to end. The slug resolves only to the caller's own
/// tenant (any other slug is `404`), the policy then checks the caller is a
/// member, and `Db::begin_tenant` runs the count as `app_api` with
/// `app.current_tenant` set, so RLS on `audit_events` filters the rows.
///
/// Real audit-event browsing belongs in the staff plane (`apps/admin`,
/// ADR-010 §I); this counts rows without exposing audit content.
#[utoipa::path(
    get,
    path = "/api/v1/tenants/{tenant}/audit-events/count",
    params(
        ("tenant" = String, Path, description = "Tenant slug per ADR-006 §C."),
    ),
    responses(
        (status = 200, description = "Audit-event count for the tenant", body = AuditEventCount),
        (status = 401, description = "No authenticated subject", body = ProblemDetails),
        (status = 403, description = "Caller is not a member; staff and system actors do not use the product API", body = ProblemDetails),
        (status = 404, description = "No tenant with that slug is visible to the caller: unknown, deleted, or another tenant's", body = ProblemDetails),
        (status = 500, description = "Internal error", body = ProblemDetails),
    ),
)]
pub async fn audit_event_count(
    Extension(db): Extension<Db>,
    Extension(subject): Extension<Subject>,
    Extension(audit_key): Extension<AuditKey>,
    method: Method,
    route: MatchedPath,
    Path(slug): Path<String>,
) -> Result<Json<AuditEventCount>, ApiError> {
    let access = DenyContext {
        db: &db,
        key: &audit_key,
        subject: &subject,
        method: &method,
        route: &route,
        action: Action::ListAuditEvents,
        requested_tenant: &slug,
    };
    let tenant = resolve_tenant(&access).await?;
    let resource = Resource {
        tenant_id: tenant.id,
        kind: ResourceKind::AuditEvent,
    };
    access.authorise(&TenantOwnership, &resource).await?;

    let count = db.tenant_audit_event_count(tenant.id).await?;

    Ok(Json(AuditEventCount { count }))
}
