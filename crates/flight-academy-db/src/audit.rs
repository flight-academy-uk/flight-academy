//! Tamper-evident audit-chain writer per ADR-004 §H + ADR-009 §C.
//!
//! Each `audit_events` row stores
//!
//! ```text
//! payload_hash = SHA-256(canonical_json(row_input) || prev_hash)
//! ```
//!
//! where:
//!
//! * `canonical_json` is RFC 8785 (JCS) over
//!   `{occurred_at, actor_class, actor_id, tenant_id, chain_kind, chain_id, payload}`.
//!   `id` and `payload_hash` are deliberately excluded from the canonical
//!   input — `id` is `gen_random_uuid` (not chain-meaningful) and
//!   `payload_hash` is the output of the hash, not an input to it.
//! * `prev_hash` is the `payload_hash` of the most recent row in the same
//!   `(chain_kind, chain_id)` chain, ordered by `(occurred_at, id)`. `NULL`
//!   for the first row in a chain. When `NULL`, the second hasher update
//!   is a no-op — the canonical input alone determines the first hash.
//!
//! ## Hash agility
//!
//! The canonical input is the persisted constituent fields, not the bytes
//! themselves. Future algorithm migration walks each chain under the new
//! `H` without storing anything additional today. The trigger event is
//! `ALTER TABLE audit_events ADD COLUMN payload_hash_algo text DEFAULT 'sha256'`
//! plus a new writer backend; the existing schema is implicitly
//! algorithm-v1 = SHA-256.
//!
//! ## Concurrency
//!
//! Two concurrent writers on the same chain would otherwise race on the
//! `prev_hash` lookup and create a fork (both reading the same prior tip,
//! both writing as if they were the next link). The writer runs at
//! `SERIALIZABLE` isolation and retries up to [`MAX_SERIALIZATION_RETRIES`]
//! times on SQLSTATE `40001` (`serialization_failure`). At our audit
//! volumes the retry rate should be near-zero except during bursts; cost
//! per retry is one round-trip plus the hash compute.

use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, Error, Result};

/// Cap on retries when SERIALIZABLE detects a write conflict. Three is
/// generous — the chain-write path is short (one SELECT, one INSERT) and
/// the conflict resolves the moment one writer commits. Higher values
/// would only delay an underlying contention problem.
const MAX_SERIALIZATION_RETRIES: usize = 3;

/// PostgreSQL SQLSTATE for `serialization_failure`. Returned when a
/// SERIALIZABLE transaction cannot commit without violating serial
/// equivalence with concurrent transactions.
const SQLSTATE_SERIALIZATION_FAILURE: &str = "40001";

/// What gets fed into `serde_jcs::to_vec` to produce the canonical bytes
/// the hash covers. Field order is the struct's declaration order, but
/// JCS canonicalisation sorts object keys lexicographically — the
/// declaration order is for readability, not protocol correctness.
#[derive(Debug, Serialize)]
struct CanonicalAuditRow<'a> {
    occurred_at: String,
    actor_class: &'a str,
    actor_id: Option<Uuid>,
    tenant_id: Option<Uuid>,
    chain_kind: &'a str,
    chain_id: Option<Uuid>,
    payload: &'a serde_json::Value,
}

/// The chain-meaningful fields of a freshly-written row. Returned so
/// callers can record the audit-id on whatever they were doing (an HTTP
/// response, a logged event) and so chain-walkers in tests can verify
/// linkage without re-querying.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub id: Uuid,
    pub occurred_at: OffsetDateTime,
    pub payload_hash: Vec<u8>,
    /// `None` when this was the first row in the chain.
    pub prev_hash: Option<Vec<u8>>,
}

impl Db {
    /// Append a row to the tenant audit chain. The chain is selected by
    /// `tenant_id`; `chain_kind` is `'tenant'`. Returns the new row's
    /// `(id, occurred_at, payload_hash, prev_hash)` — useful for response
    /// envelopes and for tests that want to verify chain linkage without
    /// re-querying.
    ///
    /// `actor_class` must be one of `"member"` / `"staff"` / `"system"`
    /// (the audit_events CHECK constraint). Invariant is enforced at the
    /// DB layer; passing anything else surfaces as an Sqlx error.
    pub async fn write_tenant_audit_event(
        &self,
        actor_class: &'static str,
        actor_id: Option<Uuid>,
        tenant_id: Uuid,
        payload: serde_json::Value,
    ) -> Result<AuditEvent> {
        for attempt in 0..MAX_SERIALIZATION_RETRIES {
            match self
                .try_write_tenant_audit_event(actor_class, actor_id, tenant_id, &payload)
                .await
            {
                Ok(ev) => return Ok(ev),
                Err(Error::Sqlx(sqlx::Error::Database(e)))
                    if e.code().as_deref() == Some(SQLSTATE_SERIALIZATION_FAILURE)
                        && attempt + 1 < MAX_SERIALIZATION_RETRIES =>
                {
                    // No backoff — the conflicting writer commits in
                    // microseconds and PG will arbitrate on the next
                    // SELECT. Re-issue immediately.
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("retry loop must return inside the loop");
    }

    async fn try_write_tenant_audit_event(
        &self,
        actor_class: &'static str,
        actor_id: Option<Uuid>,
        tenant_id: Uuid,
        payload: &serde_json::Value,
    ) -> Result<AuditEvent> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
            .execute(&mut *tx)
            .await?;

        // Read the tip of this tenant's chain. The chain index
        // (`audit_events_chain_idx` from migration 20260605000000) covers
        // `(chain_kind, chain_id, occurred_at)`; the ORDER BY uses
        // `occurred_at DESC, id DESC` so a same-microsecond tie resolves
        // deterministically (id is the within-partition tiebreaker the
        // PK includes).
        let prev_hash: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT payload_hash FROM audit_events
              WHERE chain_kind = 'tenant' AND chain_id = $1
              ORDER BY occurred_at DESC, id DESC
              LIMIT 1",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await?;

        // Generate row identifiers inside the transaction so a serialization
        // retry uses a fresh timestamp (avoids `(chain_id, occurred_at)`
        // duplicates if two retries collide on the same microsecond).
        let occurred_at: OffsetDateTime = sqlx::query_scalar("SELECT now()")
            .fetch_one(&mut *tx)
            .await?;
        let row_id = Uuid::new_v4();

        // Canonical input. RFC 3339 with UTC offset for occurred_at — a
        // deterministic byte sequence independent of pg locale or session
        // timezone. `expect` not `?` here: format on a server-generated
        // OffsetDateTime never fails; if it does we have a code bug worth
        // crashing on, not a runtime error to surface to a caller.
        let occurred_at_str = occurred_at
            .format(&time::format_description::well_known::Rfc3339)
            .expect("OffsetDateTime always RFC 3339 formattable");
        let row_input = CanonicalAuditRow {
            occurred_at: occurred_at_str,
            actor_class,
            actor_id,
            tenant_id: Some(tenant_id),
            chain_kind: "tenant",
            chain_id: Some(tenant_id),
            payload,
        };
        let canonical = serde_jcs::to_vec(&row_input)
            .expect("CanonicalAuditRow has no non-finite floats; jcs encode is infallible");

        // payload_hash = SHA-256(canonical || prev_hash).
        //   * For the first row in a chain (prev_hash = NULL), the second
        //     update is skipped — the canonical input alone determines the
        //     hash. A `NULL`-vs-`Some(&[])` ambiguity is avoided because
        //     the column stores NULL distinctly from an empty BYTEA.
        let mut hasher = Sha256::new();
        hasher.update(&canonical);
        if let Some(p) = prev_hash.as_deref() {
            hasher.update(p);
        }
        let payload_hash = hasher.finalize().to_vec();

        sqlx::query(
            "INSERT INTO audit_events
                (id, occurred_at, actor_class, actor_id, tenant_id,
                 chain_kind, chain_id, prev_hash, payload, payload_hash)
             VALUES
                ($1, $2, $3, $4, $5,
                 'tenant', $6, $7, $8, $9)",
        )
        .bind(row_id)
        .bind(occurred_at)
        .bind(actor_class)
        .bind(actor_id)
        .bind(tenant_id)
        .bind(tenant_id)
        .bind(prev_hash.as_deref())
        .bind(payload)
        .bind(&payload_hash)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(AuditEvent {
            id: row_id,
            occurred_at,
            payload_hash,
            prev_hash,
        })
    }

    /// Re-derive the `payload_hash` for a row from its persisted
    /// constituent fields. Used by chain-integrity verifiers (tests today,
    /// the periodic audit verifier per ADR-004 §H later) to detect rows
    /// whose `payload` or `prev_hash` was tampered with after insert.
    ///
    /// Inputs must be exactly the values that were inserted; the caller
    /// is responsible for SELECT-ing them with the same shape. The arity
    /// matches the row's canonical-input shape exactly — clippy's
    /// too-many-arguments lint argues for a struct, but the struct would
    /// only be used at this call boundary and `CanonicalAuditRow` already
    /// fills that role internally with the same fields.
    #[allow(clippy::too_many_arguments)]
    pub fn audit_payload_hash(
        occurred_at: OffsetDateTime,
        actor_class: &str,
        actor_id: Option<Uuid>,
        tenant_id: Option<Uuid>,
        chain_kind: &str,
        chain_id: Option<Uuid>,
        payload: &serde_json::Value,
        prev_hash: Option<&[u8]>,
    ) -> Vec<u8> {
        let occurred_at_str = occurred_at
            .format(&time::format_description::well_known::Rfc3339)
            .expect("OffsetDateTime always RFC 3339 formattable");
        let row_input = CanonicalAuditRow {
            occurred_at: occurred_at_str,
            actor_class,
            actor_id,
            tenant_id,
            chain_kind,
            chain_id,
            payload,
        };
        let canonical = serde_jcs::to_vec(&row_input)
            .expect("CanonicalAuditRow has no non-finite floats; jcs encode is infallible");
        let mut hasher = Sha256::new();
        hasher.update(&canonical);
        if let Some(p) = prev_hash {
            hasher.update(p);
        }
        hasher.finalize().to_vec()
    }
}
