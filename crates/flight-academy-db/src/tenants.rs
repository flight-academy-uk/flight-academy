//! Tenants: visible slug lookup, the audited writes, and the per-tenant
//! audit count.
//! Authorisation is decided by the caller (the ABAC layer); this module only
//! reads and writes rows.
//!
//! Soft-deleted tenants (`deleted_at IS NOT NULL`) are invisible to lookups;
//! their slug is released for reuse per ADR-007 §E.

use flight_academy_core::{TenantId, UserId};
use sqlx::{FromRow, PgConnection};

use crate::{Db, Result, audit};

/// A live tenant row: the public read shape.
#[derive(Clone, Debug, FromRow)]
pub struct Tenant {
    pub id: TenantId,
    pub slug: String,
    pub name: String,
    pub tenant_type: String,
    pub settings: serde_json::Value,
}

impl Db {
    /// The live tenant with this slug, if `viewer` may see it: today, the
    /// viewer's own tenant only. `Ok(None)` both when no live tenant has the
    /// slug and when it belongs to someone else. Visibility is part of the
    /// query rather than a check on the loaded row, so the two cases run
    /// the same statement and cannot be told apart (ADR-033). A viewer
    /// with no tenant context sees nothing.
    pub async fn visible_tenant_by_slug(
        &self,
        slug: &str,
        viewer: Option<TenantId>,
    ) -> Result<Option<Tenant>> {
        let tenant = sqlx::query_as!(
            Tenant,
            r#"SELECT id AS "id: TenantId", slug, name, tenant_type, settings
                 FROM tenants
                WHERE slug = $1
                  AND id = $2
                  AND deleted_at IS NULL"#,
            slug,
            viewer.map(|id| id.as_uuid())
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(tenant)
    }

    /// Apply a partial update and its `tenant.update` audit row in one
    /// transaction, so both commit or neither does. Fields left `None` are
    /// unchanged. `Ok(None)` when no live tenant has this id, for instance
    /// because it was soft-deleted after the caller read it.
    pub async fn update_tenant(
        &self,
        tenant_id: TenantId,
        name: Option<&str>,
        settings: Option<&serde_json::Value>,
        actor_id: UserId,
    ) -> Result<Option<Tenant>> {
        let mut tx = self.begin_audited().await?;
        let updated = update_tenant_unit(&mut tx, tenant_id, name, settings, actor_id).await?;
        tx.commit().await?;
        Ok(updated)
    }

    /// Soft-delete a tenant and write its `tenant.delete` audit row in one
    /// transaction. Returns `false`, and audits nothing, when the tenant was
    /// already deleted: the first deletion recorded the event.
    pub async fn soft_delete_tenant(
        &self,
        tenant_id: TenantId,
        reason: &str,
        actor_id: UserId,
    ) -> Result<bool> {
        let mut tx = self.begin_audited().await?;
        let deleted = soft_delete_tenant_unit(&mut tx, tenant_id, reason, actor_id).await?;
        tx.commit().await?;
        Ok(deleted)
    }

    /// Number of audit events visible in this tenant's context. Runs under
    /// RLS (`begin_tenant`), so rows of other tenants are not counted.
    pub async fn tenant_audit_event_count(&self, tenant_id: TenantId) -> Result<i64> {
        let mut tx = self.begin_tenant(tenant_id).await?;
        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!" FROM audit_events"#)
            .fetch_one(tx.conn())
            .await?;
        tx.commit().await?;
        Ok(count)
    }
}

/// The UPDATE locks the tenant row before the audit append locks the chain
/// head, the fixed order every audited write follows. `RETURNING old.*`
/// gives the values the update replaced, so the audit diff describes the
/// committed change even when another update landed after the caller read
/// the tenant.
async fn update_tenant_unit(
    conn: &mut PgConnection,
    tenant_id: TenantId,
    name: Option<&str>,
    settings: Option<&serde_json::Value>,
    actor_id: UserId,
) -> Result<Option<Tenant>> {
    let row = sqlx::query!(
        "UPDATE tenants
            SET name     = COALESCE($1, name),
                settings = COALESCE($2, settings)
          WHERE id = $3
            AND deleted_at IS NULL
        RETURNING old.name     AS old_name,
                  old.settings AS old_settings,
                  new.id, new.slug, new.name, new.tenant_type, new.settings",
        name,
        settings,
        tenant_id.as_uuid()
    )
    .fetch_optional(&mut *conn)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    let updated = Tenant {
        id: TenantId::from_uuid(row.id),
        slug: row.slug,
        name: row.name,
        tenant_type: row.tenant_type,
        settings: row.settings,
    };
    let payload = serde_json::json!({
        "action": "tenant.update",
        "changes": patch_diff(&row.old_name, &row.old_settings, &updated),
    });
    audit::write_tenant_audit_event_in_tx(conn, "member", Some(actor_id), tenant_id, &payload)
        .await?;
    Ok(Some(updated))
}

async fn soft_delete_tenant_unit(
    conn: &mut PgConnection,
    tenant_id: TenantId,
    reason: &str,
    actor_id: UserId,
) -> Result<bool> {
    let affected = sqlx::query_scalar!(
        "UPDATE tenants
            SET deleted_at      = now(),
                deletion_reason = $1
          WHERE id = $2
            AND deleted_at IS NULL
        RETURNING id",
        reason,
        tenant_id.as_uuid()
    )
    .fetch_optional(&mut *conn)
    .await?;

    if affected.is_none() {
        return Ok(false);
    }
    let payload = serde_json::json!({
        "action": "tenant.delete",
        "deletion_reason": reason,
    });
    audit::write_tenant_audit_event_in_tx(conn, "member", Some(actor_id), tenant_id, &payload)
        .await?;
    Ok(true)
}

/// The `changes` object of a `tenant.update` audit payload: each changed
/// field with its previous and new value.
fn patch_diff(
    old_name: &str,
    old_settings: &serde_json::Value,
    after: &Tenant,
) -> serde_json::Value {
    let mut changes = serde_json::Map::new();
    if old_name != after.name {
        changes.insert(
            "name".to_string(),
            serde_json::json!({ "from": old_name, "to": after.name }),
        );
    }
    if *old_settings != after.settings {
        changes.insert(
            "settings".to_string(),
            serde_json::json!({ "from": old_settings, "to": after.settings }),
        );
    }
    serde_json::Value::Object(changes)
}
