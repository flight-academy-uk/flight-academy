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
    /// DB layer; passing anything else surfaces as an Sqlx error. A
    /// type-level guard (an `ActorClass` enum) would be more Rustic;
    /// adding it requires moving `flight_academy_auth::ActorClass` to
    /// `flight-academy-core` so this crate can depend on it without a
    /// dependency cycle. Tracked for a follow-up PR; the DB CHECK is
    /// the load-bearing defence until then.
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
                    // SELECT. Re-issue immediately. Audit chain contention
                    // is operationally significant (it implies concurrent
                    // mutations on the same tenant), so the retry is
                    // logged for the operator to correlate with traffic
                    // bursts or hot-spot tenants.
                    tracing::warn!(
                        %tenant_id,
                        attempt = attempt + 1,
                        max_attempts = MAX_SERIALIZATION_RETRIES,
                        "serialization_failure on audit chain write; retrying"
                    );
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!("retry loop must return inside the loop");
    }

    /// # Pool-role invariant (applies to this helper too)
    ///
    /// This method begins its transaction via `self.pool.begin()` — the
    /// pool's session role directly, **not** `begin_tenant`. Two reasons:
    ///
    /// 1. The `prev_hash` SELECT must see all rows in the chain regardless
    ///    of the `app.current_tenant` GUC (which `begin_tenant` would set
    ///    via the `app_api` role + the RLS USING clause).
    /// 2. There is no INSERT policy on `audit_events` for `app_api`
    ///    today — an INSERT under that role would be denied.
    ///
    /// The audit writer therefore **requires** the pool's session role to
    /// be RLS-bypassing (the default for the connecting role in our
    /// development and current production paths). If the pool is ever
    /// hardened to connect as a non-RLS-bypassing role, an INSERT + SELECT
    /// policy for the audit chain writer must be added first — otherwise
    /// the `prev_hash` lookup will silently return `None` and every row
    /// will begin a new "first" entry, breaking chain integrity without
    /// any error surface. The same invariant holds for any caller of
    /// [`write_tenant_audit_event_in_tx`] — they must open their tx on a
    /// pool whose role bypasses RLS.
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
        let ev = write_tenant_audit_event_in_tx(&mut tx, actor_class, actor_id, tenant_id, payload)
            .await?;
        tx.commit().await?;
        Ok(ev)
    }
}

/// Append a tenant-chain audit row inside an existing caller-owned
/// transaction. Used by mutation handlers (PATCH, DELETE, …) that need
/// the audit row and the mutation to commit or fail together — calling
/// [`Db::write_tenant_audit_event`] alongside an UPDATE in two separate
/// transactions risks the mutation committing while the audit insert
/// fails (or vice versa), breaking the regulator-facing guarantee that
/// every state change has a corresponding audit row.
///
/// # Caller responsibilities
///
/// * Open the transaction at `SERIALIZABLE` isolation. Lower isolation
///   levels race on the `prev_hash` SELECT — two writers on the same
///   chain can read the same tip, append independently, and produce a
///   fork. `SERIALIZABLE` makes one writer retry instead.
/// * Handle retries. On SQLSTATE `40001` (serialization_failure), roll
///   back and re-run the whole UPDATE-plus-audit unit. The standalone
///   [`Db::write_tenant_audit_event`] retries internally; the in-tx
///   helper cannot, because the retry needs to re-execute the caller's
///   mutation too.
/// * Open the transaction on a pool role that bypasses RLS on
///   `audit_events` (see the pool-role invariant on
///   [`Db::try_write_tenant_audit_event`]).
pub async fn write_tenant_audit_event_in_tx(
    conn: &mut sqlx::PgConnection,
    actor_class: &'static str,
    actor_id: Option<Uuid>,
    tenant_id: Uuid,
    payload: &serde_json::Value,
) -> Result<AuditEvent> {
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
    .fetch_optional(&mut *conn)
    .await?;

    // Generate row identifiers inside the transaction so a serialization
    // retry uses a fresh timestamp (avoids `(chain_id, occurred_at)`
    // duplicates if two retries collide on the same microsecond).
    let occurred_at: OffsetDateTime = sqlx::query_scalar("SELECT now()")
        .fetch_one(&mut *conn)
        .await?;
    // UUID v4 (random) here, deliberately not v7. The audit row's
    // ordering key is `(occurred_at, id)` — the timestamp carries the
    // ordering, `id` is the within-partition tiebreaker. Leaking a
    // coarse insert timestamp into the id (as v7 does) would offer no
    // benefit and would mildly weaken the unguessability property the
    // verifier-side query relies on for partition-walk correctness.
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
    .execute(&mut *conn)
    .await?;

    Ok(AuditEvent {
        id: row_id,
        occurred_at,
        payload_hash,
        prev_hash,
    })
}

/// Re-derive the `payload_hash` for a row from its persisted constituent
/// fields. Used by chain-integrity verifiers (tests today, the periodic
/// audit verifier per ADR-004 §H later) to detect rows whose `payload` or
/// `prev_hash` was tampered with after insert.
///
/// Free function rather than `Db::` associated function — the operation
/// is pure compute with no database interaction; placing it on `Db` would
/// mislead a reader into expecting a DB round-trip.
///
/// Inputs must be exactly the values that were inserted; the caller is
/// responsible for SELECT-ing them with the same shape. The arity matches
/// the row's canonical-input shape exactly — clippy's too-many-arguments
/// lint argues for a struct, but the struct would only be used at this
/// call boundary and `CanonicalAuditRow` already fills that role
/// internally with the same fields.
#[allow(clippy::too_many_arguments)]
pub fn payload_hash(
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
