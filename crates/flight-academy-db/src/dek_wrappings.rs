//! Sqlx-backed [`KeyProvider`] implementation per ADR-023 §G — reads
//! and writes the `tenant_dek_wrappings` table introduced by migration
//! `20260620120000_tenant_dek_wrappings.sql`.
//!
//! Scope at v0.1: tenant controllers only. `user_dek_wrappings` lands
//! with the `users` table in Slice D auth; until then, the impl returns
//! [`StoreError::UnsupportedController`] for [`ControllerId::User`].
//! This is captured by ADR-023 §A's deferral note — the cross-controller
//! isolation invariant holds trivially while no user-level encrypted
//! data exists.
//!
//! The wrap layer reuses [`MasterKek`] from `flight-academy-store`; the
//! SqlxKeyProvider's responsibility is purely the storage round trip
//! (INSERT new + UPDATE retire + DELETE shred), with cryptographic
//! correctness inherited from the store crate's primitives.

use flight_academy_store::{
    ControllerId, Dek, KeyProvider, MasterKek, StoreError, StoreResult, WrappedDek,
};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use uuid::Uuid;
use zeroize::Zeroizing;

/// AES-256-GCM-SIV `algo_id` per ADR-022 §A. The wrap layer is fixed at
/// v0.1; recording it per row keeps the schema forward-compatible with
/// a future ML-KEM hybrid wrap (ADR-013 §I) without a migration.
const WRAP_ALGO_ID_GCM_SIV: i16 = 0x01;

/// Identifier of the master KEK used to wrap rows at v0.1. The `:v1`
/// suffix lets a future KEK rotation per ADR-023 §E3 increment to
/// `master:v2` without disturbing the schema.
const MASTER_KEK_ID: &str = "master:v1";

/// Sqlx-backed [`KeyProvider`] reading `tenant_dek_wrappings`.
pub struct SqlxKeyProvider {
    pool: PgPool,
    master: MasterKek,
}

impl SqlxKeyProvider {
    /// Construct from a `PgPool` and a [`MasterKek`]. The pool's session
    /// role must permit the grants attached by the
    /// `tenant_dek_wrappings` migration (SELECT/INSERT/UPDATE/DELETE).
    pub fn new(pool: PgPool, master: MasterKek) -> Self {
        Self { pool, master }
    }

    /// Project a [`ControllerId`] to a tenant UUID for SQL routing. At
    /// v0.1 only `Tenant` is supported; `User` returns a structured
    /// error so callers in higher-level crates can surface it without
    /// guessing at semantics.
    fn tenant_id(controller: ControllerId) -> StoreResult<Uuid> {
        match controller {
            ControllerId::Tenant(uuid) => Ok(uuid),
            ControllerId::User(_) => Err(StoreError::UnsupportedController {
                reason: "user_dek_wrappings — users table lands in Slice D auth",
            }),
        }
    }
}

impl KeyProvider for SqlxKeyProvider {
    async fn generate_dek(&self, controller: ControllerId, record_kind: &str) -> StoreResult<u32> {
        let tenant_id = Self::tenant_id(controller)?;

        // Random 32-byte DEK from OsRng per ADR-022 §G. Zeroizing
        // scrubs the plaintext bytes when this scope exits regardless
        // of which branch the function takes.
        let mut dek_bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut dek_bytes[..]);

        let mut tx = self.pool.begin().await.map_err(sqlx_to_store)?;

        // ADR-023 §A's partial unique index `tenant_dek_wrappings_one_active`
        // is the structural guard against creating a second active row;
        // we check explicitly inside the transaction so the error
        // surface is the trait-level `AlreadyActiveDek` rather than a
        // raw 23505 unique-violation.
        let active_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
                SELECT 1 FROM tenant_dek_wrappings
                 WHERE tenant_id = $1 AND record_kind = $2 AND state = 'active'
             )",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_to_store)?;
        if active_exists {
            return Err(StoreError::AlreadyActiveDek);
        }

        // Compute the next version number for this (tenant, record_kind).
        // COALESCE handles the first-version-ever case.
        let max_version: Option<i32> = sqlx::query_scalar(
            "SELECT MAX(dek_version)
               FROM tenant_dek_wrappings
              WHERE tenant_id = $1 AND record_kind = $2",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .fetch_one(&mut *tx)
        .await
        .map_err(sqlx_to_store)?;
        let version: u32 = max_version.map_or(1, |n| (n as u32) + 1);

        let wrapped = self
            .master
            .wrap(controller, record_kind, version, &dek_bytes)?;

        sqlx::query(
            "INSERT INTO tenant_dek_wrappings
                (tenant_id, record_kind, dek_version, wrapped_bytes,
                 wrap_algo_id, kek_id, state)
             VALUES ($1, $2, $3, $4, $5, $6, 'active')",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .bind(version as i32)
        .bind(wrapped.as_bytes())
        .bind(WRAP_ALGO_ID_GCM_SIV)
        .bind(MASTER_KEK_ID)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_to_store)?;

        tx.commit().await.map_err(sqlx_to_store)?;
        Ok(version)
    }

    async fn active_dek_for(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(Dek, u32)> {
        let tenant_id = Self::tenant_id(controller)?;

        let row: Option<(i32, Vec<u8>)> = sqlx::query_as(
            "SELECT dek_version, wrapped_bytes
               FROM tenant_dek_wrappings
              WHERE tenant_id = $1 AND record_kind = $2 AND state = 'active'",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_to_store)?;

        let (version_i32, wrapped_bytes) = row.ok_or(StoreError::NoActiveDek)?;
        let version = version_i32 as u32;
        let wrapped = WrappedDek::from_bytes(wrapped_bytes);
        let dek = self
            .master
            .unwrap(controller, record_kind, version, &wrapped)?;
        Ok((dek, version))
    }

    async fn dek_at_version(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<Dek> {
        let tenant_id = Self::tenant_id(controller)?;

        let row: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT wrapped_bytes
               FROM tenant_dek_wrappings
              WHERE tenant_id = $1 AND record_kind = $2 AND dek_version = $3",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .bind(dek_version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_to_store)?;

        let wrapped_bytes = row.ok_or(StoreError::NoSuchDekVersion {
            version: dek_version,
        })?;
        let wrapped = WrappedDek::from_bytes(wrapped_bytes);
        self.master
            .unwrap(controller, record_kind, dek_version, &wrapped)
    }

    async fn rotate_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
    ) -> StoreResult<(u32, u32)> {
        let tenant_id = Self::tenant_id(controller)?;

        let mut dek_bytes = Zeroizing::new([0u8; 32]);
        OsRng.fill_bytes(&mut dek_bytes[..]);

        // Atomic per ADR-023 §E1: retire the prior active and insert
        // the new active under one transaction. The partial unique
        // index allows at most one active row, so the UPDATE→INSERT
        // ordering is load-bearing — INSERT-then-UPDATE would
        // transiently violate the index.
        let mut tx = self.pool.begin().await.map_err(sqlx_to_store)?;

        let retired_version: Option<i32> = sqlx::query_scalar(
            "UPDATE tenant_dek_wrappings
                SET state = 'retired', retired_at = now()
              WHERE tenant_id = $1 AND record_kind = $2 AND state = 'active'
          RETURNING dek_version",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .fetch_optional(&mut *tx)
        .await
        .map_err(sqlx_to_store)?;
        let retired_version = retired_version.ok_or(StoreError::NoActiveDek)? as u32;

        let new_version = retired_version + 1;
        let wrapped = self
            .master
            .wrap(controller, record_kind, new_version, &dek_bytes)?;

        sqlx::query(
            "INSERT INTO tenant_dek_wrappings
                (tenant_id, record_kind, dek_version, wrapped_bytes,
                 wrap_algo_id, kek_id, state)
             VALUES ($1, $2, $3, $4, $5, $6, 'active')",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .bind(new_version as i32)
        .bind(wrapped.as_bytes())
        .bind(WRAP_ALGO_ID_GCM_SIV)
        .bind(MASTER_KEK_ID)
        .execute(&mut *tx)
        .await
        .map_err(sqlx_to_store)?;

        tx.commit().await.map_err(sqlx_to_store)?;
        Ok((new_version, retired_version))
    }

    async fn shred_dek(
        &self,
        controller: ControllerId,
        record_kind: &str,
        dek_version: u32,
    ) -> StoreResult<()> {
        let tenant_id = Self::tenant_id(controller)?;

        // Check existence + state first so the trait-level error
        // surface (NoSuchDekVersion vs CannotShredActiveDek) matches
        // what InMemoryKeyProvider reports.
        let row_state: Option<String> = sqlx::query_scalar(
            "SELECT state
               FROM tenant_dek_wrappings
              WHERE tenant_id = $1 AND record_kind = $2 AND dek_version = $3",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .bind(dek_version as i32)
        .fetch_optional(&self.pool)
        .await
        .map_err(sqlx_to_store)?;

        let state = row_state.ok_or(StoreError::NoSuchDekVersion {
            version: dek_version,
        })?;
        if state == "active" {
            return Err(StoreError::CannotShredActiveDek);
        }

        sqlx::query(
            "DELETE FROM tenant_dek_wrappings
              WHERE tenant_id = $1 AND record_kind = $2 AND dek_version = $3",
        )
        .bind(tenant_id)
        .bind(record_kind)
        .bind(dek_version as i32)
        .execute(&self.pool)
        .await
        .map_err(sqlx_to_store)?;
        Ok(())
    }
}

fn sqlx_to_store(e: sqlx::Error) -> StoreError {
    StoreError::Storage(format!("sqlx: {e}"))
}
