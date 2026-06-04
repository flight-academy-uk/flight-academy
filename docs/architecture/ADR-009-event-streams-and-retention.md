# ADR-009 — Domain-event streams, audit scope, and retention

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | events, audit, retention, partitioning, compliance, outbox, webhooks, sizing |
| **Supersedes** | (none — refines [ADR-001 §C](ADR-001-platform.md), see B) |

## Context

Earlier ADRs each made a sound local decision and named what looks like four
streams: the transactional outbox
([ADR-003 §D](ADR-003-db-migrations.md)), `audit_events`
([ADR-004 §D](ADR-004-defence-in-depth.md)), the auth-event stream
([ADR-004 §C](ADR-004-defence-in-depth.md)), and webhook deliveries
([ADR-006 §H](ADR-006-api-contract.md) /
[ADR-007 §B2](ADR-007-sync-filtering-deletion.md)). Read together, three
problems surface that none of the source ADRs answers:

1. **Their relationship is undefined.** Without an organising decision they
   grow inconsistent.
2. **Combined write volume is unsized.** ADR-001 §C says *every* Permit and
   Deny is audited; combined with the hash chain and per-state-change outbox
   that's billions of rows on a single-writer chain.
3. **Long-term retention shape is undecided.** ADR-004 §D names 7 years and
   points at S3 Object Lock; the live cost, cold-tier format, per-stream
   defaults, and pruning are open.

Constraints: UK CAA/EASA expect auditable history; GDPR forbids covert PII
in audit (already resolved by opaque IDs + crypto-shred-severable
identifiability); solo maintainer on small ARM nodes — the hot path cannot
pay full-fidelity auditing on every read; self-host parity must degrade
without losing tamper-evidence (PG + MinIO + parquet + duckdb); restraint
([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) 35–36) — reuse PG + outbox +
hash chain + S3, heavier machinery only when these can't carry the load;
truth (instrument 28) — say what's logged, what isn't, and where it goes.

## Decision

**Three independently-purposed stores — `audit_events`, `events_outbox`,
`auth_events` — with webhooks as an outbox dispatcher. Audit-write scope
narrowed to *sensitive Permits + every Deny + every elevated action*.
Per-tenant + per-user + platform hash chains. Monthly range partitioning.
Per-stream retention with a documented hot → warm → cold tier path that
reaches Parquet on object storage.**

### A. Stream architecture — three stores + a dispatcher

| Store | Purpose | Tamper-evident? | Lifetime |
| --- | --- | --- | --- |
| `audit_events` | Compliance: who did what, why, when. Regulator-readable. | Yes (hash chain + S3 Object Lock) | 7y, then cold-tier (D) |
| `events_outbox` | Reliable cross-system delivery committed in the same tx as the state change. | No (delivery, not a fact source) | 90d, dropped (D) |
| `auth_events` | Forensic: login/lockout/passkey-added record separate from ops logs. | No (longer retained for forensic reach) | 2y, archived (D) |
| Webhook deliveries | Per-subscriber dispatch tracking on top of `events_outbox`. | No | 30d (D) |

**Webhooks are not a fifth store** — they're one of the outbox handlers
([ADR-003 §D](ADR-003-db-migrations.md): notifications, integration dispatch,
webhooks). A `webhook_deliveries` table tracks attempts/signatures/retry
state; the durable fact lives in the outbox. A subscriber can replay within
the outbox window; beyond it, recovery uses `updated_since`
([ADR-007 §B1](ADR-007-sync-filtering-deletion.md)).

The three stores stay separate because their requirements actively conflict:
audit's INSERT-only + hash chain is incompatible with the deletes the outbox
needs after dispatch; auth's longer forensic retention would force
audit-grade durability on every login attempt; outbox in audit contaminates
the regulator-readable record with delivery state.

### B. Audit scope — sensitive Permits + every Deny (refines ADR-001 §C)

[ADR-001 §C](ADR-001-platform.md) reads "every Permit recorded with subject,
action, resource, decision rationale; every Deny with the reason." Applied
literally that's a write on every list-row read of a fleet view — billions of
rows on a serialised hash chain.

**This ADR refines §C** so that an audit row is written when:

- **`Permit`** — only if the resource carries at least one field above
  *operational* class per
  [ADR-008 §B](ADR-008-data-sharing-posture.md). Reading the fleet doesn't
  audit; reading medical detail does; reading the member roster does.
- **Every `Deny`** — regardless of class. Small in volume, forensically
  essential.
- **Every elevated action** — regardless of class: bulk-sync reads
  ([ADR-008 §D](ADR-008-data-sharing-posture.md)), safety-reporter de-anon
  ([ADR-001 §G](ADR-001-platform.md)), key unwraps
  ([ADR-001 §D](ADR-001-platform.md)), staff cross-tenant access
  ([ADR-010](ADR-010-platform-operator-access.md)), key rotation,
  configuration changes.

The Accepted §C text is read through this paragraph — same reconciliation
pattern as ADR-005's `fa-*` table and ADR-006 §C's `/me/…` paragraph. Net
effect: ~100× audit-volume reduction without losing the access regulators
ask about, since the access they ask about *is* the sensitive/elevated kind.

### C. Hash-chain partitioning — per-tenant + per-user + platform

A global chain would serialise every audit write platform-wide. Decision:

- **One chain per tenant** for actions in a `tenants/{tenant}/…` context.
- **One chain per user** for actions on `me/…` resources (per
  [ADR-006 §C](ADR-006-api-contract.md)).
- **One chain for the platform plane** (staff actions per
  [ADR-010](ADR-010-platform-operator-access.md), `actor_type='admin'|'system'`).

Each chain stores `prev_hash` referencing the prior row *in that chain* and
verifies independently; jobs walk chains in parallel. Cross-chain ordering
is `occurred_at` (UTC) — chains are parallel histories, not a global log.

### D. Retention defaults + tiered storage

| Stream | Hot (live PG) | Warm (PG, compressed) | Cold (object store, Parquet) | Archive (Object Lock) |
| --- | --- | --- | --- | --- |
| `audit_events` | 90 days | 1 year | 5 years | until 7y, then permanent deletion |
| `events_outbox` | dispatched + 90 days | — | — | (no cold tier — not a fact source) |
| `auth_events` | 30 days | 2 years | — | — |
| `webhook_deliveries` | 30 days | — | — | — |

**Cold tier for `audit_events`.** Monthly partitions (E) older than the warm
window export to Apache Parquet on object storage with S3 Object Lock in
compliance mode. Tamper-evidence travels two ways: each partition's manifest
carries the *last hash* of every chain that contributed rows, and a signed
Merkle checkpoint per partition export ties the partition's content hash to
the issuer key. Cold queries use DuckDB or Athena.

**Ended-chain archival.** When a tenant or user is erased mid-month, the
chain ends cleanly with the erasure event row. The partition still archives
on its month boundary — the cold-tier export job runs on time, not on chain
state — and the ended chain's last hash sits in the partition manifest as
the verifiable end of that history.

`events_outbox` does **not** archive. The durable history is in
`audit_events` (state-change Permits) and the operational tables
(`updated_at` and tombstones,
[ADR-007](ADR-007-sync-filtering-deletion.md)). Outbox rows hard-delete once
dispatched and the replay-buffer window has expired.

### E. Monthly range partitioning

`audit_events` and `events_outbox` are declarative range-partitioned by month
on `occurred_at` / `created_at`. Pruning is a partition drop — constant cost
regardless of table size — which makes the tier shape in D practical.

Addition follows expand-contract ([ADR-003 §B](ADR-003-db-migrations.md)):
add the next-month partition before the month begins (scheduled migration);
drop expired partitions only after the cold-tier export is verified.

### F. Event abstraction lives in `flight-academy-core::events`

Domain-event types, serialisation, and the `OutboxHandler` trait live in
`flight-academy-core` ([ADR-005 §C](ADR-005-workspace-layout.md)). Handlers
(notifications, webhook dispatch, integration adapters) live in their own
crates and implement the trait. Splits to `flight-academy-events` only if
the module grows past ~200 lines.

### G. The event catalog is not specified here

ADR-009 covers stores, scope, partitioning, retention. Event names
(`booking.created`, `aircraft.aog`, `resource.deleted`, `resource.erased`,
…) and payloads live in code + emitted spec
([ADR-006 §A](ADR-006-api-contract.md)). The naming shape (`resource.event`,
past tense) and signing/retry contract stay in
[ADR-006 §H](ADR-006-api-contract.md).

## Consequences

**Positive.** Volumes are sized — ~100× audit reduction from B; hot-tier
bounded by D + E; cold-tier cheap (Parquet 10–30× compression vs JSON).
Tamper-evidence travels across tiers (per-tenant chains hot; Parquet
manifest + signed Merkle checkpoint cold). Per-tenant chains parallelise.
Self-host parity (PG + MinIO + Parquet + DuckDB ≡ PG + S3 + Athena). Reuses
what exists. One taxonomy, three roles.

**Negative.** Audit scope is a code-level concern — a serialiser must know
each field's class; mitigated by typing fields with their class. Cold-tier
export is operational work — failure mode is "live partition retains, storage
grows" (acceptable). Per-stream + per-tier retention is a configuration
matrix more complex than a single number. The Merkle checkpoint mechanism
adds a small surface (signing key, verification job).

**Neutral.** Narrowing §C gives up visibility into routine operational
reads by default — Notes name scale-up paths to get it back. The outbox
isn't archived: the durable history lives in `audit_events` + resource
tables. Webhook deliveries are short-lived; subscribers needing long history
use `updated_since`.

## Alternatives considered

- **One unified event store.** Folding audit + outbox + auth into one log
  conflicts irreducibly: INSERT-only chain vs deletes-after-dispatch, audit
  durability vs every-login retention. Three stores cost less.
- **Every `Permit` audited (literal §C).** Maximum compliance posture but
  cost-prohibitive on hot paths. **Available as scale-up** when needed
  (Notes triggers): *D-alt-1* sampled full visibility (reservoir sampling N%
  of operational Permits — statistical, not tamper-evident per row); *D-alt-2*
  operational-log capture to cold tier (full coverage, unsigned JSONL/Parquet,
  Object-Lock for tamper-evidence at object level rather than row).
- **Sampled audit instead of class-based.** Rejected as default — regulators
  ask yes/no questions ("did this user access this medical record?"). Sampling
  fits the routine tier (D-alt-1), not the sensitive one.
- **Chain-partitioning variants** (scale-up options):
  - *C-alt-1* per-(tenant, month) chains — composes with E's monthly
    partitioning; chain ends at month boundary; bounded length, parallel
    verification, drop-partition prunes both rows and chain. **Likely the
    first scale-up.**
  - *C-alt-2* per-(tenant, resource-type) chains — more within-tenant
    parallelism; ordering across resource types becomes ambiguous.
  - *C-alt-3* hierarchical Merkle-of-chains with published signed root
    (Certificate-Transparency / Sigstore Rekor pattern) — global cross-tenant
    integrity proofs at the cost of one signature per epoch.
  - *C-alt-4* skip-pointer chains — log-time verification of any prefix;
    orthogonal refinement once chains get long.
- **Storage-tier variants** (scale-up):
  - *D-alt-3* Iceberg/Delta Lake over Parquet — transactional catalog,
    schema evolution, time-travel. Worth it when cold query volume or schema
    change pace grows.
  - *D-alt-4* TimescaleDB hypertables for warm — keeps everything in PG with
    ~10–20× compression; adds a PG extension dependency (attractive for
    self-hosters who already run it).
  - *D-alt-5* Debezium / pg_logical-replication → Kafka/ClickHouse/warehouse
    — heavy CDC; adopted when a real-time analytics or warehouse need
    materialises. ADR-003 §D anticipates this.
  - *D-alt-6* AWS Glacier Deep Archive past 7y — cheapest credible store if
    retention extends; hours-to-restore.
- **Partitioning variants** (scale-up):
  - *E-alt-1* hash by `tenant_id` — colocates a tenant's rows; useful if a
    very large tenant skews monthly partitions.
  - *E-alt-2* multi-level (month → tenant) — many partitions, more overhead.
  - *E-alt-3* per-tenant tables — strongest isolation, breaks at scale.
    Not chosen.
  - *E-alt-4* TimescaleDB hypertables — automatic chunking + compression
    (see D-alt-4).
- **Webhooks as a fifth store.** Rejected — a webhook delivery is a *side
  effect* of an event in the outbox, not a separate fact.
  `webhook_deliveries` carries delivery state without claiming to be a fact
  source.

## References

- [ADR-001 §C/§D/§E/§G/§H](ADR-001-platform.md) — ABAC (refined in B); crypto-shred + safety key; webhook idempotency + DPA; safety de-anon (elevated action per B); no-telemetry.
- [ADR-003 §D](ADR-003-db-migrations.md) — outbox (A's foundation); expand-contract (E); role separation.
- [ADR-004 §B/§C/§D](ADR-004-defence-in-depth.md) — no-PII rule; auth-event stream (sized here); hash-chained `audit_events` (partitioned in C, tiered in D).
- [ADR-005 §C](ADR-005-workspace-layout.md) — event abstraction in `flight-academy-core` (F).
- [ADR-006 §A/§H](ADR-006-api-contract.md) — emitted-spec source of truth for event names (G); webhook naming on top of A's dispatcher.
- [ADR-007 §B/§D](ADR-007-sync-filtering-deletion.md) — `updated_since` as post-window recovery; erasure events flow through outbox.
- [ADR-008 §B/§D](ADR-008-data-sharing-posture.md) — data-class taxonomy B reads; bulk reads always audited.
- [ADR-010 §E](ADR-010-platform-operator-access.md) — populates the platform chain (C); elevation context as structured metadata.
- [ADR-011](ADR-011-user-consent-grant.md) — grants/refresh-rotations/revocations write to the per-user chain (C); refresh-reuse is a security event in the auth stream (A).
- [ADR-012 §F](ADR-012-cross-tenant-dek-erasure.md) — audit chains unaltered by erasure (rows reference opaque IDs only); `resource.reference-erased` flows through outbox.
- [domain-model §4 / §5](../design/domain-model.md) — sensitivity tiers (B's class check); cross-cutting mechanisms.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (truth); 35–36 (restraint); 38 (diligence); 48 (watchfulness).
- Parquet — <https://parquet.apache.org/>; Iceberg — <https://iceberg.apache.org/>; TimescaleDB (now Tiger Data) — <https://www.tigerdata.com/>; Debezium — <https://debezium.io/>; S3 Object Lock — <https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html>; Certificate Transparency — <https://certificate.transparency.dev/>; Sigstore Rekor — <https://docs.sigstore.dev/logging/overview/>; PG partitioning — <https://www.postgresql.org/docs/current/ddl-partitioning.html>; DuckDB — <https://duckdb.org/>.

## Notes

**Scale-up triggers** — what would make us move:

- **Audit scope (B).** D-alt-1 (sampling) when posture demands "every
  decision" but statistical is enough. D-alt-2 (operational log to cold tier)
  when "every decision, durable, queryable" is required. Both off-default,
  deliberate operator opt-in.
- **Chains (C).** C-alt-1 (per-tenant, per-month) when chains grow long
  enough for verification to be a concern — likely the first scale-up,
  natural with E. C-alt-2 if within-tenant throughput on a single chain
  bottlenecks. C-alt-3 if a regulator demands cross-tenant cryptographic
  integrity proofs.
- **Cold tier (D).** D-alt-3 (Iceberg) when cold-tier query volume or schema
  change grows. D-alt-4 (TimescaleDB) only as a self-host-local choice.
  D-alt-5 (event bridging) when a real analytics or warehouse need
  materialises — outbox is positioned to feed it.
- **Partitioning (E).** E-alt-1 (hash by tenant) if a very large tenant
  skews monthly partitions.

The load-bearing decision is B — the audit-scope refinement of ADR-001 §C.
It changes platform cost by orders of magnitude without losing what
regulators care about. Everything else is mechanism in service of B and of
ADR-007's sync semantics having a durable backbone to ride on.
