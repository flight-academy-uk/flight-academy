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
use flight_academy_db::{Db, Tenant, audit};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ProblemDetails};

/// Cap on retries when the write transaction hits SERIALIZABLE
/// serialization_failure (SQLSTATE 40001). Matches the audit writer's
/// standalone-mode retry cap; the audit writer can't retry on our
/// behalf in in-tx mode because the retry must re-execute the caller's
/// UPDATE as well.
const MAX_WRITE_RETRIES: usize = 3;
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";

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
/// UPDATE and the audit-event INSERT run in the same SERIALIZABLE
/// transaction so they commit or fail together — see ADR-009 §A on the
/// regulator-facing guarantee that every state change has a paired audit
/// row.
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

    // Retry on SERIALIZABLE conflict only; other errors propagate. The
    // audit writer cannot retry on its own behalf in in-tx mode — it has
    // no view of our UPDATE and would leave a half-applied state on
    // conflict. `try_tenant_patch_once` returns `Option<Tenant>`: `None`
    // means the tenant was concurrently soft-deleted between the slug
    // resolution (outside the tx) and the UPDATE (inside the tx) — that
    // surfaces as 404 here, matching what the next read would have seen.
    let updated = retry_serializable(
        || try_tenant_patch_once(&db, &subject, &tenant, &req),
        tenant.id,
    )
    .await?
    .ok_or(Error::NotFound { resource: "tenant" })?;
    Ok(Json(updated.into()))
}

/// Run `op` inside a retry loop that re-attempts on SQLSTATE 40001
/// (`serialization_failure`). Up to [`MAX_WRITE_RETRIES`] attempts; any
/// other sqlx error propagates immediately. The factory pattern lets each
/// attempt build a fresh transaction and re-run the caller's UPDATE.
async fn retry_serializable<T, F, Fut>(mut op: F, tenant_id: Uuid) -> Result<T, ApiError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, flight_academy_db::Error>>,
{
    for attempt in 0..MAX_WRITE_RETRIES {
        match op().await {
            Ok(v) => return Ok(v),
            Err(flight_academy_db::Error::Sqlx(sqlx::Error::Database(e)))
                if e.code().as_deref() == Some(SQLSTATE_SERIALIZATION_FAILURE)
                    && attempt + 1 < MAX_WRITE_RETRIES =>
            {
                tracing::warn!(
                    %tenant_id,
                    attempt = attempt + 1,
                    max_attempts = MAX_WRITE_RETRIES,
                    "serialization_failure on tenant write; retrying"
                );
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!("retry loop must return inside the loop");
}

async fn try_tenant_patch_once(
    db: &Db,
    subject: &Subject,
    tenant: &Tenant,
    req: &TenantPatchRequest,
) -> Result<Option<Tenant>, flight_academy_db::Error> {
    let mut tx = db.pool().begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await?;

    // Apply UPDATE conditionally on which fields are present. Use a
    // single statement with COALESCE so the audit chain sees one event
    // regardless of how many fields changed. `fetch_optional` (not
    // `fetch_one`) so a concurrent soft-delete between resolve_tenant
    // and now returns `Ok(None)` → 404, not a `RowNotFound` → 500.
    let updated: Option<Tenant> = sqlx::query_as(
        "UPDATE tenants
            SET name     = COALESCE($1, name),
                settings = COALESCE($2, settings)
          WHERE id = $3
            AND deleted_at IS NULL
        RETURNING id, slug, name, tenant_type, settings",
    )
    .bind(req.name.as_deref())
    .bind(req.settings.as_ref())
    .bind(tenant.id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(updated) = updated else {
        // Concurrent soft-delete won the race. Commit the empty tx
        // (nothing to write, nothing to audit) and surface NotFound to
        // the caller.
        tx.commit().await?;
        return Ok(None);
    };

    let changes = patch_diff(tenant, &updated);
    let payload = serde_json::json!({
        "action": "tenant.update",
        "changes": changes,
    });
    audit::write_tenant_audit_event_in_tx(
        &mut tx,
        "member",
        Some(subject.user_id),
        tenant.id,
        &payload,
    )
    .await?;

    tx.commit().await?;
    Ok(Some(updated))
}

fn patch_diff(before: &Tenant, after: &Tenant) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    if before.name != after.name {
        changes.insert(
            "name".to_string(),
            serde_json::json!({ "from": before.name, "to": after.name }),
        );
    }
    if before.settings != after.settings {
        changes.insert(
            "settings".to_string(),
            serde_json::json!({ "from": before.settings, "to": after.settings }),
        );
    }
    serde_json::Value::Object(changes)
}

/// DELETE a tenant by slug — soft-delete (`deleted_at` + `deletion_reason`).
/// Tenant-admin only (TenantAdministration). The UPDATE and the audit
/// row commit or fail together for the same regulator-facing reason as
/// PATCH.
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

    retry_serializable(
        || try_tenant_delete_once(&db, &subject, &tenant, reason),
        tenant.id,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn try_tenant_delete_once(
    db: &Db,
    subject: &Subject,
    tenant: &Tenant,
    reason: &str,
) -> Result<(), flight_academy_db::Error> {
    let mut tx = db.pool().begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
        .execute(&mut *tx)
        .await?;

    // Idempotency guard: if the tenant was concurrently soft-deleted,
    // `deleted_at IS NULL` makes this a no-op. RETURNING id lets us tell
    // the difference between "we deleted it" and "nothing matched".
    let affected: Option<Uuid> = sqlx::query_scalar(
        "UPDATE tenants
            SET deleted_at      = now(),
                deletion_reason = $1
          WHERE id = $2
            AND deleted_at IS NULL
        RETURNING id",
    )
    .bind(reason)
    .bind(tenant.id)
    .fetch_optional(&mut *tx)
    .await?;

    // No row → the tenant was already deleted by a concurrent request.
    // Audit nothing (the original deletion already recorded the event)
    // and return 204 anyway so the client sees an idempotent answer.
    if affected.is_none() {
        tx.commit().await?;
        return Ok(());
    }

    let payload = serde_json::json!({
        "action": "tenant.delete",
        "deletion_reason": reason,
    });
    audit::write_tenant_audit_event_in_tx(
        &mut tx,
        "member",
        Some(subject.user_id),
        tenant.id,
        &payload,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}
