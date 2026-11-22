//! Tenant resource handlers. Owns the public wire shape (`TenantResponse`)
//! and the shared slug → row lookup (`resolve_tenant`) consumed by every
//! slug-keyed handler — including the audit-events sibling resource.

use axum::{
    Extension, Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use flight_academy_auth::{
    Action, Decision, Policy, Resource, ResourceAttributes, ResourceKind, Subject,
    TenantAdministration, TenantOwnership,
};
use flight_academy_core::Error;
use flight_academy_db::{Db, Tenant};
use serde::{Deserialize, Serialize};
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

/// Partial-update body for `PATCH /api/v1/tenants/{tenant}`. Either field
/// is optional; omitted fields are left untouched. `slug` and
/// `tenant_type` are intentionally not editable — both are identity-shaped
/// (URL stability, regulatory operator class) and changes deserve their
/// own endpoints with separate semantics.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct TenantPatchRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub settings: Option<serde_json::Value>,
}

impl TenantPatchRequest {
    fn is_noop(&self) -> bool {
        self.name.is_none() && self.settings.is_none()
    }
}

/// Soft-delete body for `DELETE /api/v1/tenants/{tenant}`. `deletion_reason`
/// is required so every deletion carries an auditable rationale. Free-form
/// string for now; the enum form lands with the retention-rules resource.
#[derive(Debug, Deserialize, ToSchema)]
pub struct TenantDeleteRequest {
    pub deletion_reason: String,
}

/// PATCH a tenant by slug. Tenant-admin only (TenantAdministration). The
/// update and its audit row commit together; see `Db::update_tenant`.
#[utoipa::path(
    patch,
    path = "/api/v1/tenants/{tenant}",
    params(
        ("tenant" = String, Path, description = "Tenant slug per ADR-006 §C."),
    ),
    request_body = TenantPatchRequest,
    responses(
        (status = 200, description = "Updated tenant profile", body = TenantResponse),
        (status = 400, description = "Validation failed", body = ProblemDetails),
        (status = 401, description = "No authenticated subject", body = ProblemDetails),
        (status = 403, description = "Subject is not a tenant-admin of this tenant", body = ProblemDetails),
        (status = 404, description = "No live tenant with that slug", body = ProblemDetails),
        (status = 500, description = "Internal error", body = ProblemDetails),
    ),
)]
pub async fn tenant_patch(
    Extension(db): Extension<Db>,
    Extension(subject): Extension<Subject>,
    Path(slug): Path<String>,
    Json(req): Json<TenantPatchRequest>,
) -> Result<Json<TenantResponse>, ApiError> {
    let tenant = resolve_tenant(&db, &slug).await?;
    let resource = Resource {
        tenant_id: tenant.id,
        kind: ResourceKind::Tenant,
        owner: None,
        attributes: ResourceAttributes,
    };
    match TenantAdministration.permit(&subject, Action::UpdateTenant, &resource) {
        Decision::Permit => {}
        Decision::Deny { .. } | Decision::NotApplicable => return Err(Error::Forbidden.into()),
    }

    // No-op PATCH: echo the current state, do not emit an audit event.
    // Auditing a request that changed nothing would pollute the chain
    // with rows that the regulator's diff-replay can't account for.
    if req.is_noop() {
        return Ok(Json(tenant.into()));
    }

    // Validate name length matches the DB CHECK so the failure surfaces
    // as 400 not 500 (the DB CHECK would otherwise raise SQLSTATE 23514
    // which maps to Internal).
    if let Some(name) = &req.name {
        let len = name.chars().count();
        if !(1..=200).contains(&len) {
            return Err(Error::Validation {
                field: "name",
                message: "must be 1..=200 characters".to_string(),
            }
            .into());
        }
    }

    // `None`: the tenant was soft-deleted after `resolve_tenant` read it;
    // that surfaces as 404, as the next read would.
    let updated = db
        .update_tenant(
            &tenant,
            req.name.as_deref(),
            req.settings.as_ref(),
            subject.user_id,
        )
        .await?
        .ok_or(Error::NotFound { resource: "tenant" })?;
    Ok(Json(updated.into()))
}

/// DELETE a tenant by slug — soft-delete (`deleted_at` + `deletion_reason`).
/// Tenant-admin only (TenantAdministration). The deletion and its audit
/// row commit together; see `Db::soft_delete_tenant`.
#[utoipa::path(
    delete,
    path = "/api/v1/tenants/{tenant}",
    params(
        ("tenant" = String, Path, description = "Tenant slug per ADR-006 §C."),
    ),
    request_body = TenantDeleteRequest,
    responses(
        (status = 204, description = "Tenant soft-deleted"),
        (status = 400, description = "deletion_reason missing or empty", body = ProblemDetails),
        (status = 401, description = "No authenticated subject", body = ProblemDetails),
        (status = 403, description = "Subject is not a tenant-admin of this tenant", body = ProblemDetails),
        (status = 404, description = "No live tenant with that slug", body = ProblemDetails),
        (status = 500, description = "Internal error", body = ProblemDetails),
    ),
)]
pub async fn tenant_delete(
    Extension(db): Extension<Db>,
    Extension(subject): Extension<Subject>,
    Path(slug): Path<String>,
    Json(req): Json<TenantDeleteRequest>,
) -> Result<Response, ApiError> {
    let tenant = resolve_tenant(&db, &slug).await?;
    let resource = Resource {
        tenant_id: tenant.id,
        kind: ResourceKind::Tenant,
        owner: None,
        attributes: ResourceAttributes,
    };
    match TenantAdministration.permit(&subject, Action::DeleteTenant, &resource) {
        Decision::Permit => {}
        Decision::Deny { .. } | Decision::NotApplicable => return Err(Error::Forbidden.into()),
    }

    let reason = req.deletion_reason.trim();
    if reason.is_empty() {
        return Err(Error::Validation {
            field: "deletion_reason",
            message: "must be non-empty".to_string(),
        }
        .into());
    }
    // Cap on the free-form deletion_reason. The string lands in the audit
    // event payload; without an upper bound, a tenant-admin could push
    // an arbitrarily large value into that tenant's audit chain. 1000
    // chars is generous for the human-rationale shape (a short paragraph)
    // while bounding the per-row cost.
    if reason.chars().count() > 1000 {
        return Err(Error::Validation {
            field: "deletion_reason",
            message: "must be at most 1000 characters".to_string(),
        }
        .into());
    }

    // `false` means it was already deleted by a concurrent request; the
    // answer is the same idempotent 204.
    db.soft_delete_tenant(tenant.id, reason, subject.user_id)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
