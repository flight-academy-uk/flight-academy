//! Tenants: slug lookup, the audited writes, and the per-tenant audit count.
//! Authorisation is decided by the caller (the ABAC layer); this module only
//! reads and writes rows.
//!
//! Soft-deleted tenants (`deleted_at IS NOT NULL`) are invisible to lookups;
//! their slug is released for reuse per ADR-007 §E.

use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

use crate::{Db, Result, audit};

/// A live tenant row: the public read shape.
#[derive(Clone, Debug, FromRow)]
pub struct Tenant {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub tenant_type: String,
    pub settings: serde_json::Value,
}

impl Db {
    /// Resolve a public tenant slug to its row. `Ok(None)` when no live
    /// tenant has that slug. Runs outside any tenant-scoped transaction,
    /// because the caller's tenant context is not known until it resolves.
    pub async fn tenant_by_slug(&self, slug: &str) -> Result<Option<Tenant>> {
        let tenant = sqlx::query_as!(
            Tenant,
            "SELECT id, slug, name, tenant_type, settings
               FROM tenants
              WHERE slug = $1
                AND deleted_at IS NULL",
            slug
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(tenant)
    }

    /// Apply a partial update and its `tenant.update` audit row in one
    /// SERIALIZABLE transaction, so both commit or neither does. Fields left
    /// `None` are unchanged. `Ok(None)` when the tenant was soft-deleted
    /// after `before` was read.
    pub async fn update_tenant(
        &self,
        before: &Tenant,
        name: Option<&str>,
        settings: Option<&serde_json::Value>,
        actor_id: Uuid,
    ) -> Result<Option<Tenant>> {
        self.serializable("tenant update", before.id, |mut tx| {
            Box::pin(async move {
                let result = update_tenant_unit(&mut tx, before, name, settings, actor_id).await;
                (tx, result)
            })
        })
        .await
    }

    /// Soft-delete a tenant and write its `tenant.delete` audit row in one
    /// SERIALIZABLE transaction. Returns `false`, and audits nothing, when
    /// the tenant was already deleted: the first deletion recorded the event.
    pub async fn soft_delete_tenant(
        &self,
        tenant_id: Uuid,
        reason: &str,
        actor_id: Uuid,
    ) -> Result<bool> {
        self.serializable("tenant delete", tenant_id, |mut tx| {
            Box::pin(async move {
                let result = soft_delete_tenant_unit(&mut tx, tenant_id, reason, actor_id).await;
                (tx, result)
            })
        })
        .await
    }

    /// Number of audit events visible in this tenant's context. Runs under
    /// RLS (`begin_tenant`), so rows of other tenants are not counted.
    pub async fn tenant_audit_event_count(&self, tenant_id: Uuid) -> Result<i64> {
        let mut tx = self.begin_tenant(tenant_id).await?;
        let count = sqlx::query_scalar!(r#"SELECT COUNT(*) AS "count!" FROM audit_events"#)
            .fetch_one(tx.conn())
            .await?;
        tx.commit().await?;
        Ok(count)
    }
}

async fn update_tenant_unit(
    conn: &mut PgConnection,
    before: &Tenant,
    name: Option<&str>,
    settings: Option<&serde_json::Value>,
    actor_id: Uuid,
) -> Result<Option<Tenant>> {
    let updated = sqlx::query_as!(
        Tenant,
        "UPDATE tenants
            SET name     = COALESCE($1, name),
                settings = COALESCE($2, settings)
          WHERE id = $3
            AND deleted_at IS NULL
        RETURNING id, slug, name, tenant_type, settings",
        name,
        settings,
        before.id
    )
    .fetch_optional(&mut *conn)
    .await?;

    let Some(updated) = updated else {
        return Ok(None);
    };
    let payload = serde_json::json!({
        "action": "tenant.update",
        "changes": patch_diff(before, &updated),
    });
    audit::write_tenant_audit_event_in_tx(conn, "member", Some(actor_id), before.id, &payload)
        .await?;
    Ok(Some(updated))
}

async fn soft_delete_tenant_unit(
    conn: &mut PgConnection,
    tenant_id: Uuid,
    reason: &str,
    actor_id: Uuid,
) -> Result<bool> {
    let affected = sqlx::query_scalar!(
        "UPDATE tenants
            SET deleted_at      = now(),
                deletion_reason = $1
          WHERE id = $2
            AND deleted_at IS NULL
        RETURNING id",
        reason,
        tenant_id
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
