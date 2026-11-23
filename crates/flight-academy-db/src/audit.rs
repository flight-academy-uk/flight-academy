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
//! * `prev_hash` is the `payload_hash` of the previous row appended to the
//!   same `(chain_kind, chain_id)` chain, as recorded in the chain's
//!   `audit_chain_heads` row. `NULL` for the first row in a chain. When `NULL`, the second hasher update
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
//! Each chain has one `audit_chain_heads` row holding its tip. An append
//! locks that row (`SELECT ... FOR UPDATE`) at READ COMMITTED, so
//! concurrent appends to one chain queue and each one links to the row the
//! previous lock holder committed. None fails for contention, and the chain
//! cannot fork. Appends to different chains do not wait for each other.
//!
//! `occurred_at` is `clock_timestamp()` read while the head is locked, so
//! within a chain it increases in append order unless the database clock
//! steps backwards. It is not used to find the tip.
//!
//! Every audited write locks its domain rows first and the chain head
//! last, which keeps the lock order fixed and the head held only for the
//! append itself.

use flight_academy_core::{TenantId, UserId};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Db, Error, Result};

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

/// The chain an audit row is appended to (ADR-009 §C). Each has its own
/// `audit_chain_heads` row, so appends to different chains never wait for
/// each other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Chain {
    /// A tenant's chain. Its rows carry `tenant_id`, so RLS shows them in
    /// that tenant's context.
    Tenant(TenantId),
    /// A user's own chain, for events with no tenant context.
    User(UserId),
}

impl Chain {
    fn kind(self) -> &'static str {
        match self {
            Self::Tenant(_) => "tenant",
            Self::User(_) => "user",
        }
    }

    fn id(self) -> Uuid {
        match self {
            Self::Tenant(id) => id.as_uuid(),
            Self::User(id) => id.as_uuid(),
        }
    }

    fn tenant_id(self) -> Option<Uuid> {
        match self {
            Self::Tenant(id) => Some(id.as_uuid()),
            Self::User(_) => None,
        }
    }
}

impl Db {
    /// Append a row to `chain` in its own transaction. See
    /// [`Db::write_tenant_audit_event`] for the arguments and the pool-role
    /// invariant, which apply unchanged.
    pub async fn write_audit_event(
        &self,
        chain: Chain,
        actor_class: &'static str,
        actor_id: Option<UserId>,
        payload: serde_json::Value,
    ) -> Result<AuditEvent> {
        let mut tx = self.begin_audited().await?;
        let event = append_in_tx(&mut tx, chain, actor_class, actor_id, &payload).await?;
        tx.commit().await?;
        Ok(event)
    }

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
    ///
    /// # Pool-role invariant
    ///
    /// The transaction comes from `Db::begin_audited`, which begins it on
    /// the pool's session role directly, **not** through `begin_tenant`.
    /// Two reasons:
    ///
    /// 1. `audit_chain_heads` has row-level security enabled and no policy,
    ///    so only a role that bypasses RLS can read or advance a head.
    /// 2. There is no INSERT policy on `audit_events` for `app_api` today,
    ///    so an INSERT under that role would be denied.
    ///
    /// The audit writer therefore **requires** the pool's session role to
    /// bypass RLS (checked at startup by [`verify_pool_role`]). If the pool
    /// is ever hardened to connect as a role that does not bypass RLS, an
    /// INSERT and SELECT policy for the audit chain writer must be added
    /// first. The same holds for every caller of
    /// [`write_tenant_audit_event_in_tx`]: [`Db::update_tenant`] and
    /// [`Db::soft_delete_tenant`] run it inside `Db::begin_audited` for
    /// this reason.
    pub async fn write_tenant_audit_event(
        &self,
        actor_class: &'static str,
        actor_id: Option<UserId>,
        tenant_id: TenantId,
        payload: serde_json::Value,
    ) -> Result<AuditEvent> {
        self.write_audit_event(Chain::Tenant(tenant_id), actor_class, actor_id, payload)
            .await
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
/// * Open the transaction at READ COMMITTED, as `Db::begin_audited`
///   does. Under REPEATABLE READ or SERIALIZABLE, a writer that waited for
///   the chain head would fail with a serialization error when the holder
///   commits.
/// * Call this after locking or changing the domain rows, as the last
///   statement before commit, so the chain head is locked last and held
///   only for the append.
/// * Open the transaction on a pool role that bypasses RLS (see the
///   pool-role invariant on
///   [`Db::write_tenant_audit_event`]).
pub async fn write_tenant_audit_event_in_tx(
    conn: &mut sqlx::PgConnection,
    actor_class: &'static str,
    actor_id: Option<UserId>,
    tenant_id: TenantId,
    payload: &serde_json::Value,
) -> Result<AuditEvent> {
    append_in_tx(
        conn,
        Chain::Tenant(tenant_id),
        actor_class,
        actor_id,
        payload,
    )
    .await
}

/// Append a row to `chain` inside a caller-owned transaction, under the
/// same caller responsibilities as [`write_tenant_audit_event_in_tx`].
async fn append_in_tx(
    conn: &mut sqlx::PgConnection,
    chain: Chain,
    actor_class: &'static str,
    actor_id: Option<UserId>,
    payload: &serde_json::Value,
) -> Result<AuditEvent> {
    // Audit rows and their hash input hold bare UUIDs: the canonical row is
    // part of the chain format and must not change with the wire form.
    let actor_id = actor_id.map(|id| id.as_uuid());
    let (chain_kind, chain_id, tenant_id) = (chain.kind(), chain.id(), chain.tenant_id());

    // Lock this chain's head, creating it on the chain's first append.
    // Under READ COMMITTED a writer that waits here resumes with the head
    // as the previous holder committed it.
    let head = lock_chain_head(conn, chain_kind, chain_id).await?;
    let prev_hash = head.tip_hash;

    // Read after the lock, so a writer that began earlier but waited does
    // not record a time older than the row it links to.
    let occurred_at: OffsetDateTime = sqlx::query_scalar!(r#"SELECT clock_timestamp() AS "now!""#)
        .fetch_one(&mut *conn)
        .await?;

    // UUID v4 (random) here, deliberately not v7. The row's primary key is
    // `(occurred_at, id)`: the timestamp already carries the time, and `id`
    // only breaks ties within a partition. Putting a coarse insert time
    // into the id as well (as v7 does) would add nothing and would make
    // ids guessable.
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
        tenant_id,
        chain_kind,
        chain_id: Some(chain_id),
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

    sqlx::query!(
        "INSERT INTO audit_events
            (id, occurred_at, actor_class, actor_id, tenant_id,
             chain_kind, chain_id, prev_hash, payload, payload_hash)
         VALUES
            ($1, $2, $3, $4, $5,
             $6, $7, $8, $9, $10)",
        row_id,
        occurred_at,
        actor_class,
        actor_id,
        tenant_id,
        chain_kind,
        chain_id,
        prev_hash.as_deref(),
        payload,
        &payload_hash
    )
    .execute(&mut *conn)
    .await?;

    sqlx::query!(
        "UPDATE audit_chain_heads
            SET seq              = seq + 1,
                tip_hash         = $3,
                last_occurred_at = $4
          WHERE chain_kind = $1 AND chain_id = $2",
        chain_kind,
        chain_id,
        &payload_hash,
        occurred_at
    )
    .execute(&mut *conn)
    .await?;

    Ok(AuditEvent {
        id: row_id,
        occurred_at,
        payload_hash,
        prev_hash,
    })
}

/// The tip of a chain, read from its locked `audit_chain_heads` row.
struct ChainHead {
    /// `None` while the chain has no rows.
    tip_hash: Option<Vec<u8>>,
}

/// Lock a chain's head row until the transaction ends, creating the row
/// first if this is the chain's first append. Two first appends racing to
/// create it both end up locking the same row: the loser's
/// `ON CONFLICT DO NOTHING` waits for the winner and then does nothing.
async fn lock_chain_head(
    conn: &mut sqlx::PgConnection,
    chain_kind: &str,
    chain_id: Uuid,
) -> Result<ChainHead> {
    loop {
        let head = sqlx::query_as!(
            ChainHead,
            "SELECT tip_hash FROM audit_chain_heads
              WHERE chain_kind = $1 AND chain_id = $2
                FOR UPDATE",
            chain_kind,
            chain_id
        )
        .fetch_optional(&mut *conn)
        .await?;
        if let Some(head) = head {
            return Ok(head);
        }
        sqlx::query!(
            "INSERT INTO audit_chain_heads (chain_kind, chain_id)
             VALUES ($1, $2)
             ON CONFLICT (chain_kind, chain_id) DO NOTHING",
            chain_kind,
            chain_id
        )
        .execute(&mut *conn)
        .await?;
    }
}

/// Verify that the given connection's session role can drive the audit
/// chain writer. Designed to be called once at process startup against a
/// connection acquired from the same pool the writer will use.
///
/// Four capabilities are checked, against `current_user` on `conn`:
///
/// 1. **INSERT grant** on `audit_events` — without this the writer's
///    INSERT fails immediately with a SQL error. Loud failure at first
///    write; checking at startup turns it into refuse-to-start.
/// 2. **SELECT grant** on `audit_events`, which the chain verifier and the
///    tenant audit count read. Same loud-failure shape as INSERT.
/// 3. **SELECT, INSERT and UPDATE grants** on `audit_chain_heads`, which
///    every append locks, creates on a chain's first write, and advances.
/// 4. **RLS bypass** — `rolbypassrls OR rolsuper` on the row in
///    `pg_roles` matching `current_user`. `audit_chain_heads` has RLS
///    enabled and forced with no policy, so without bypass every head
///    lookup finds nothing and the first INSERT of a head is refused.
///
/// PG semantics note: role attributes (BYPASSRLS, SUPERUSER) do not
/// inherit through `GRANT role TO role` — only the role's own row in
/// `pg_roles` matters. Checking `current_user` directly is therefore
/// correct.
///
/// Failures produce [`Error::AuditPoolRoleUnfit`] carrying which
/// specific capabilities were missing, so the operator log line is
/// actionable.
pub async fn verify_pool_role(conn: &mut sqlx::PgConnection) -> Result<()> {
    let row = sqlx::query!(
        r#"SELECT
             current_user::text AS "role!",
             has_table_privilege(current_user, 'audit_events', 'INSERT') AS "can_insert!",
             has_table_privilege(current_user, 'audit_events', 'SELECT') AS "can_select!",
             has_table_privilege(current_user, 'audit_chain_heads', 'SELECT, INSERT, UPDATE')
                 AS "can_write_heads!",
             COALESCE(
                 (SELECT rolbypassrls OR rolsuper FROM pg_roles WHERE rolname = current_user),
                 false
             ) AS "bypasses_rls!""#
    )
    .fetch_one(&mut *conn)
    .await?;
    let (role, can_insert, can_select, can_write_heads, bypasses_rls) = (
        row.role,
        row.can_insert,
        row.can_select,
        row.can_write_heads,
        row.bypasses_rls,
    );

    if can_insert && can_select && can_write_heads && bypasses_rls {
        return Ok(());
    }
    Err(Error::AuditPoolRoleUnfit {
        role,
        can_insert,
        can_select,
        can_write_heads,
        bypasses_rls,
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
