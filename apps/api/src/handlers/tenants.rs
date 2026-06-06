//! Tenant resource handlers. Owns the public wire shape (`TenantResponse`)
//! and the shared slug → row lookup (`resolve_tenant`) consumed by every
//! slug-keyed handler — including the audit-events sibling resource.

use axum::{Extension, Json, extract::Path};
use flight_academy_auth::{
    Action, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Subject, TenantOwnership,
};
use flight_academy_core::Error;
use flight_academy_db::{Db, Tenant};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ProblemDetails};

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
pub async fn resolve_tenant(db: &Db, slug: &str) -> Result<Tenant, ApiError> {
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
pub async fn tenant_get(
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
