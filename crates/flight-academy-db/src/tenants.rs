//! tenants — slug → Tenant lookup. The public lookup surface for the
//! first operational entity per ADR-001 §A. Authorization is left to the
//! ABAC layer (per ADR-001 §C): this module only resolves identity.
//!
//! Soft-deleted tenants (deleted_at IS NOT NULL) are invisible to lookups;
//! their slug is released for reuse per ADR-007 §E.

use crate::{Db, Result};
use sqlx::FromRow;
use uuid::Uuid;

/// A live tenant row. Fields mirror the public read shape; sensitive
/// columns (DEK wrapping, etc.) land with the auth-keys PR per
/// ADR-013 / ADR-001 §D.
#[derive(Clone, Debug, FromRow)]
pub struct Tenant {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub tenant_type: String,
    pub settings: serde_json::Value,
}

impl Db {
    /// Resolve a public tenant slug to its full row. Returns `Ok(None)`
    /// when no live tenant has that slug — soft-deleted rows are
    /// invisible. Runs outside any tenant-scoped transaction because the
    /// caller's tenant context isn't known until after this resolves.
    pub async fn tenant_by_slug(&self, slug: &str) -> Result<Option<Tenant>> {
        let tenant = sqlx::query_as::<_, Tenant>(
            "SELECT id, slug, name, tenant_type, settings
               FROM tenants
              WHERE slug = $1
                AND deleted_at IS NULL",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await?;
        Ok(tenant)
    }
}
