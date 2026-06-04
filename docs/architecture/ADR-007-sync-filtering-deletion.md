# ADR-007 — Incremental sync, list filtering, and deletion semantics

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | api, sync, filtering, deletion, gdpr, webhooks, contract |
| **Supersedes** | (none) |

## Context

[ADR-006](ADR-006-api-contract.md) fixed the static contract but left
*filtering* and *incremental sync* unspecified; per-endpoint ad-hoc filters
and no defined recovery path would follow. The motivating case: a consumer's
pipeline fails for a window (April–May) and must re-fetch every record that
*changed* in it — including ones created earlier but **modified or deleted**
during it. A naive "created between those dates" query misses both.

Already available: the transactional outbox
([ADR-003 §D](ADR-003-db-migrations.md)); HMAC-signed webhooks
([ADR-006 §H](ADR-006-api-contract.md)); crypto-shred erasure
([ADR-001 §D](ADR-001-platform.md)); the no-PII rule for logs and audit
([ADR-004 §B/§D](ADR-004-defence-in-depth.md)); cursor pagination with unique
tiebreaker ([ADR-006 §D](ADR-006-api-contract.md)).

Constraints: GDPR erasure must coexist with deletion-sync (tombstones must
never become covert PII); the mechanism must reuse what exists (outbox,
`updated_at`, RLS); the surface is a public promise; expand-contract
([ADR-003](ADR-003-db-migrations.md)) makes retrofitting `updated_at` /
soft-delete onto a replicated resource painful **and** a silent break for
consumers already mirroring — the durable shape must be decided now
([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instruments 38, 35–36).

Best-practice surveys (AIP-132/135/164/165; Stripe's eventual-consistency and
30-day retention pitfalls) shaped two findings: prefer a strictly monotonic
cursor over raw timestamps for an event stream; handle deletions as
soft-deletes that flow through the same "changed since" feed.

*What* may be synced, *for whom*, under *what consent* is
[ADR-008](ADR-008-data-sharing-posture.md)'s concern; this ADR is the
mechanism.

## Decision

**Replicated list resources expose a curated indexed filter set; consumers
stay in sync via a never-expiring `updated_since` resource feed (recovery)
plus real-time outbox-dispatched webhooks (low latency); deletions are
soft-deletes that flow through the feed as no-PII tombstones; GDPR erasure is
a distinct crypto-shred-backed event, not a soft-delete.**

### A. Curated indexed filter set — no arbitrary search

Each API-exposed list resource publishes a small documented filter set; every
filter is backed by an index. Unlisted fields are not filterable. Shape
follows AIP-132. Business-time filters (`occurred_at`, `start_at`,
`posted_at`, `date`) answer domain queries; the sync cursor (B) is separate.

| Resource | Filters (besides `cursor`, `limit`) |
| --- | --- |
| bookings | `from`/`to` (start_at), `aircraft_id`, `instructor_id`, `status`, `updated_since` |
| flight-logs | date range, `aircraft`, solo/dual, `updated_since` |
| aircraft | `status`, `updated_since` |
| maintenance-jobs | `status`, `aircraft`, `owner`, `updated_since` |
| safety/occurrences | `status`, `severity`, `updated_since` |
| members | `role`, `status`, `updated_since` |
| transactions | date range (posted_at), `type`, `updated_since` |

A consumer needing an unexposed filter requests it; we add filter + index
together, rather than carrying unbounded query cost.

### B. Incremental sync — two complementary paths

**B1 — `updated_since` resource feed (durable recovery backbone).** Every
replicated resource carries `updated_at`. `GET …/<resource>?updated_since=<watermark>`
returns every record changed at or after the watermark — **including tombstones**
(C) — ordered `(updated_at, id)`, paged with the §D cursor. Reads live tables
→ **never expires**; recovers from any watermark.

The cursor encodes `(updated_at, id)` so equal timestamps cannot cause skips;
boundary comparison is tuple-`>` lexicographic
(`updated_at > w_ts OR (updated_at = w_ts AND id > w_id)`).

**Commit-order safety (the silent skip).** `updated_at` is set at write
time; rows become visible at *commit* time. A long-running transaction can
set `updated_at = T1` and commit at `T5` after a consumer has already read
past T1. Mitigation: **serve only up to `now() - δ`**, a safe-lag watermark
covering the longest expected write transaction (tens of seconds in
practice). Query: `updated_at <= now() - δ AND (updated_at, id) > (w_ts, w_id)`.
The exact `δ` lives in the operations runbook; that *some* safe lag is
applied is part of the contract.

**`updated_at` write discipline.** Set by a Postgres `BEFORE UPDATE` trigger
to `now()` on every row update — including the soft-delete bump (C) and any
undelete (which also clears `deleted_at`). Trigger-driven so it's consistent
across the API write path, bulk DML, and migration backfills, and a developer
cannot forget to bump it.

**B2 — Webhooks (real-time, low latency).** Emitted from the outbox
([ADR-003 §D](ADR-003-db-migrations.md)); at-least-once; HMAC-signed;
`event_id` dedupe; named `resource.event` per
[ADR-006 §H](ADR-006-api-contract.md).

**B3 — Event replay endpoint (deferred).** Optional
`GET …/events?since=<seq>` replaying the outbox's strictly-monotonic sequence
for consumers preferring event catch-up over resource diffing. **Not built
initially** — B1 + B2 already cover recovery and real-time. If built, must
serve only up to a safe high-water mark (in-flight transactions cannot be
skipped).

Guidance: **webhooks for "as it happens"; `updated_since` for "catch me up /
recover."**

### C. Deletions — soft-delete that flows through the feed

Replicated resources soft-delete: `deleted_at` timestamp + `deletion_reason`
category. Soft-deleting bumps `updated_at`, so the deletion appears in the
`updated_since` feed (B1) and fires `resource.deleted` (B2) — one mechanism,
no separate deletions stream. The feed returns the tombstone, reduced to
`{ id, deleted_at, deletion_reason }`. Semantics follow AIP-164; undelete
only where a workflow needs it.

**Tombstones carry opaque ID + `deleted_at` + reason category only — never
PII**, mirroring the audit-row rule
([ADR-004 §D](ADR-004-defence-in-depth.md)). A tombstone is an existence
signal, not a data store.

A full-id reconciliation export is an optional drift backstop, deferred;
incremental sync is the primary mechanism.

### D. GDPR erasure is not a soft-delete

Operational deletion (C) and statutory erasure are different events.

- **Right to erasure (GDPR Art. 17)** crypto-shreds the owning DEK
  ([ADR-001 §D](ADR-001-platform.md)), rendering encrypted content
  unrecoverable, and emits a distinct **`resource.erased`** event so a mirror
  can shred its copy. The erased record's representation is the minimal
  pseudonymous shell retention rules require.
- We **surface** erasure in the feed honestly
  ([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instrument 28). We **cannot**
  technically force a third party to delete its mirror — that obligation is
  contractual via the sub-processor / DPA terms
  ([ADR-001 §E](ADR-001-platform.md)).
- Erasure events, like tombstones, carry no PII.

Distinguishing `deleted` from `erased` lets a consumer treat a cancelled
booking differently from an Art. 17 request.

**Cross-tenant erasure** (e.g. B's maintenance record references A's aircraft;
A is erased) is specified in
[ADR-012](ADR-012-cross-tenant-dek-erasure.md): controller-owner DEK rule +
opaque references + a new `resource.reference-erased` event. B's record
survives; the reference becomes a dangling pseudonym. This ADR's surface is
unchanged.

### E. Indexing

Tenant/user-leading composites aligned to the cursor sort, so RLS scope +
filter + pagination collapse into one index seek:

- `(tenant_id, updated_at, id)` — or `(user_id, updated_at, id)` for
  user-owned resources — serves the `updated_since` feed.
- `(tenant_id, <business_time>, id)` per business-time filter.
- Partial indexes for low-cardinality status filters and the default
  not-deleted view (`… WHERE deleted_at IS NULL`).
- The outbox PK already serves B3.

Index additions follow [ADR-003](ADR-003-db-migrations.md) (`CREATE INDEX
CONCURRENTLY` in a non-transactional migration with its recovery note).

### F. Scope boundary

Applies to **API-exposed list resources that support sync** — see the
bounded-context resource maps under [domain-model §2](../design/domain-model.md#2-resource--operation-map-by-bounded-context).
Internal-only or singleton/config resources are out of scope: don't
add `updated_at`, soft-delete, or feed machinery where nothing syncs.

The mechanism is bounded by the posture in
[ADR-008](ADR-008-data-sharing-posture.md): the feed exposes only
**operational, tenant-owned** data to a third-party key (under an elevated,
tenant-granted, audited sync scope); **user-owned personal** data syncs only
under the owning user's consent
([ADR-011](ADR-011-user-consent-grant.md)); **special-category** data never
appears in the feed at all. First-party clients use the same primitive under
first-party credentials.

## Consequences

**Positive.** Recovery is real and unbounded — any watermark, no Stripe-style
retention cliff. Deletions stop being invisible: soft-delete-as-change closes
the silent drift gap. One shape (creates, updates, deletions) on one feed and
one webhook catalog. GDPR coexists honestly with replication. No covert PII
store. Cheap — reuses outbox, `updated_at`, RLS, §D cursor, §H webhooks.

**Negative.** Schema discipline from the first migration (`updated_at`
trigger + `deleted_at` + `deletion_reason` + composite indexes on every
replicated resource). Soft-deleted rows persist; default queries must filter
`deleted_at IS NULL`. Two sync paths to document (feed vs webhooks).
Eventual-consistency care: both `updated_at` (B1) and the outbox sequence
(B3) can be assigned before commit; same mitigation (safe-lag watermark, then
tuple comparison on B1 / strictly monotonic sequence on B3).

**Neutral.** Curated filters give up arbitrary querying — by design.
Soft-delete interacts with uniqueness constraints; use partial unique
indexes (`… WHERE deleted_at IS NULL`). Two new event kinds
(`resource.deleted`, `resource.erased`) join the catalog.

## Alternatives considered

- **`updated_at` as the event-stream cursor.** Rejected for the *event
  stream*: timestamps tie and skew; monotonic log position is the best
  practice. Kept for the resource feed (B1) where it indexes the live table.
- **Hard delete + separate deletions feed.** Splits the change stream; loses
  `updated_at` ordering. Soft-delete-as-change keeps one ordered stream.
- **Periodic full reconciliation only.** Expensive, high-latency, can't say
  *when*. Retained only as an optional backstop on top of incremental sync.
- **Arbitrary field filtering / query DSL.** Unbounded cost, index sprawl,
  unstable contract. AIP-132 curated set is restrained.
- **Fold into ADR-006 §D.** This is a distinct decision with its own
  alternatives and a GDPR reconciliation; one decision per ADR.

## References

- [ADR-001 §D/§E/§G](ADR-001-platform.md) — crypto-shred (D), DPA propagation, retention shells.
- [ADR-003 §D](ADR-003-db-migrations.md) — outbox; expand-contract; index migrations.
- [ADR-004 §B/§D](ADR-004-defence-in-depth.md) — no-PII rule for logs and audit, applied to tombstones (C) and erasure (D).
- [ADR-006 §D/§H](ADR-006-api-contract.md) — cursor + tiebreaker (B1); webhook naming/signing/delivery (B2).
- [ADR-008](ADR-008-data-sharing-posture.md) — bounds what the feed exposes (F).
- [ADR-009](ADR-009-event-streams-and-retention.md) — sizes outbox retention (B2 replay window); confirms B1 as post-window recovery.
- [ADR-010](ADR-010-platform-operator-access.md) — staff plane out of product API.
- [ADR-011](ADR-011-user-consent-grant.md) — user-owned syncs via user-consent scopes.
- [ADR-012](ADR-012-cross-tenant-dek-erasure.md) — cross-tenant erasure (D).
- [domain-model §2.13 / §4](../design/domain-model.md) — surface this applies to; sensitivity tiers.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (truth), 35–36 (restraint), 38 (diligence), 48 (watchfulness).
- AIPs 132/135/164/165 — <https://google.aip.dev/>.

## Notes

Load-bearing reconciliation is D: operational delete (soft, reversible) and
statutory erasure (crypto-shred, irreversible) are different events.
Collapsing them either understates erasure or overstates every cancellation.

Schema invariants — `updated_at` trigger + soft-delete + composite indexes —
lock from the first migration. B3 replay and the reconciliation export are
deferred until a consumer need is shown.
