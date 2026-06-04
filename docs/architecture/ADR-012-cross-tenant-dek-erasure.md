# ADR-012 — Cross-tenant DEK assignment and erasure-by-reference semantics

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | encryption, envelope, dek, gdpr, erasure, cross-tenant, maintenance, audit |
| **Supersedes** | (none — resolves cross-tenant cases left open by [ADR-001 §D](ADR-001-platform.md) and [ADR-007 §D](ADR-007-sync-filtering-deletion.md)) |

## Context

[ADR-001 §D](ADR-001-platform.md) decided envelope encryption with
per-tenant + per-user DEKs and named Art. 17 erasure as crypto-shredding
the owning DEK. [ADR-007 §D](ADR-007-sync-filtering-deletion.md)
distinguished operational delete from statutory erasure and flagged a real
edge case:

> Tenant B (Part-145 / CAMO) authors maintenance records about tenant A's
> aircraft via a maintenance grant (domain-model §3, §2.3). Whose DEK
> encrypts the encrypted columns? What happens when A exercises Art. 17?

Same question, three other shapes: a pilot's flight-log entry (user-owned)
carrying a signoff from a tenant-A instructor; a pilot's competency evidence
logged inside tenant A's training session; a safety occurrence referencing
B's CAMO via the maintenance link.

Failure modes if undecided: wrong tenant's DEK chosen (A's erasure
destroys B's regulatory record); references dangle ambiguously (consumers
can't reconcile); audit chain interaction undefined (A's chain ends; B's
chain references A — meaning unspecified).

Constraints: each controller keeps its own regulated records (GDPR + CAA/
EASA §145.A.55); erasure must remain effective (a pilot mustn't be
reconstructable from B's mentions of them); references must be honest
(consumers learn explicitly); audit chains stay tamper-evident (no row
alteration on erasure); restraint
([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) 35–36) — solve this with the
existing envelope + event machinery, no new infrastructure.

## Decision

**Every encrypted row is encrypted under the DEK of its *owning controller*.
Cross-tenant references are stored as opaque IDs — no DEK crosses the
boundary. When an entity is erased, references to it survive in other
controllers' records as *dangling pseudonyms*, and a new
`resource.reference-erased` event ships so mirrors reconcile. Audit chains
preserve their reference rows unchanged because the references were opaque
to begin with.**

### A. Controller-owner rule for DEK assignment

For every encrypted column on every row, the DEK is the **owning
controller's**:

| Record | DEK owner | Rationale |
| --- | --- | --- |
| Tenant-owned operational (fleet, booking, schedule) | the owning tenant | unchanged from ADR-001 §D |
| `maintenance_record` authored by B against A's tail | **tenant B** | B is the Part-145 controller (EASA §145.A.55) |
| `maintenance_job` worklist at B | tenant B | same |
| `signoff` on a `flight_log_entry` (A's instructor signs pilot's flight) | **the pilot (user-owned)** with a pseudonymous tenant reference | signoff is part of the pilot's portable record |
| `competency_evidence` (graded by A) | **the pilot (user-owned)** | competency is portable; evidence is content of that profile |
| `safety_occurrence` (operational) | the reporting tenant | unchanged; reporter identity stays under the separate safety key (ADR-001 §G) |

Rule: *the controller of the record is the DEK owner.* A record may
**reference** other controllers' entities by opaque ID but never *contains*
their encrypted content. **The DEK never crosses the boundary.**

`flight-academy-store`'s `KeyProvider`
([ADR-005 §C](ADR-005-workspace-layout.md)) gains a
`for_record(record_kind, controller)` resolver; application code reads
plaintext through `EncryptedString` / `EncryptedJson` wrappers and never
picks the DEK itself.

### B. Cross-tenant references are opaque, not encrypted-cross-DEK

A `maintenance_record` (controlled by B) points at A's aircraft by the
opaque external ID `ac_<base32>`
([ADR-006 §D](ADR-006-api-contract.md)) — a string, not encrypted, not
under any DEK. Resolution goes through A's `aircraft` table at read time
with the API checking the caller has access and the entity exists (C).

Consequences: no DEK crosses controllers (sharing DEKs would be a covert
escape from the per-controller boundary); a reference is a *claim* the
entity exists in another controller's domain (truth checked at resolution
time, claim itself not encrypted); A's crypto-shred does not affect B's
reference *string* — the text persists but resolution starts returning
"deleted entity" (C).

### C. Erasure leaves the reference as a dangling pseudonym + new event

When entity X (a tenant, a user) is erased per
[ADR-007 §D](ADR-007-sync-filtering-deletion.md):

1. X's DEK is crypto-shredded; X's content is unrecoverable; X's own rows
   become tombstones and a `resource.erased` event ships.
2. Other controllers' rows that **reference** X (B's maintenance records
   pointing at A's `ac_…`) are **not modified**. The reference string
   persists; B's encrypted columns under B's DEK remain readable.
3. **A new event class — `resource.reference-erased`** — ships to every
   webhook consumer of B's data subscribed to it:

   ```json
   {
     "type": "resource.reference-erased",
     "resource_type": "maintenance_record",
     "id": "mr_…",
     "reference_field": "aircraft_id",
     "reference_value": "ac_…",
     "referenced_controller_kind": "tenant",
     "reason": "controller-erased",
     "occurred_at": "…"
   }
   ```

   No PII (opaque IDs only — [ADR-004 §D](ADR-004-defence-in-depth.md)).
4. The API resolver for a dangling reference returns a stable shape:

   ```json
   { "id": "ac_…", "status": "deleted", "deleted_at": "…", "deletion_reason": "controller-erased" }
   ```

**Direct references only.** `resource.reference-erased` fires for *direct*
foreign references in the record. If B's record references C's record
which references A, A's erasure fires the event for *C*'s record (the
direct holder of the reference); B's mirror reconciles transitively by
re-resolving C if it cares — propagating an event through every chain of
references would be unbounded and noisy. One hop, one event.

This is the *honesty* property: B's record survives, A's erasure is
effective, consumers learn explicitly.

### D. Maintenance-grant lifecycle on erasure

The `maintenance_grant` is the operator↔CAMO contract that lets B
maintain A's tail. When A is erased:

- every active grant `(A, B)` is **terminated** in the same transaction as
  A's erasure;
- `grant.terminated` fires to B with `reason: "operator-erased"`;
- B's historical records (jobs, signoffs, tech-log) **stay intact** with
  dangling references; the grant termination is the authoritative signal
  that no further work will be authored.

Symmetrically when B is erased: A's view of "we had a CAMO relationship
with B" terminates, B's records become unreadable under B's shredded DEK,
A's historical grant rows keep dangling pseudonyms to `ten_<B>`.

### E. User-owned data crossing a tenant erasure

**E1 — user-owned record references an erased tenant.** A pilot's
`flight_log_entry` is encrypted under the *pilot's* DEK with a
`tenant_context` field and a `signoff` carrying `instructor_user_id` +
`instructor_tenant_id`. Tenant erased: encrypted content unaffected (it's
under the user's DEK); `tenant_id` references become dangling pseudonyms;
the log entry keeps functioning. The signoff's `instructor_user_id`
references the *instructor* (a user), not the tenant — if the instructor's
account survives, the signoff resolves normally; if it's also erased, that
reference dangles too.

**E2 — competency evidence logged in a tenant's session.** Evidence is
part of the pilot's portable profile (user-owned); the evidence row is
encrypted under the *pilot's* DEK with a pseudonymous tenant reference,
**not** the tenant's DEK. This is the controller-owner rule applied to a
record that "feels tenant-side" but is actually user-side. A pilot leaves
an ATO, the ATO disappears, the competency history travels with the pilot;
references to the disappeared ATO dangle as opaque "former tenant" IDs.

### F. Audit chain interaction

Per [ADR-009 §C](ADR-009-event-streams-and-retention.md), `audit_events`
rows reference actors and resources by opaque UUID with no PII. Erasure
therefore **does not modify any audit row**:

- A's per-tenant chain ends at the erasure event; the partition archives
  on schedule
  ([ADR-009 §D](ADR-009-event-streams-and-retention.md)).
- B's per-tenant chain continues; rows historically referencing A's
  entities (a Bristol audit row recording "viewed aircraft `ac_…` of
  tenant `ten_<A>` under elevation `el_…`") remain as written — opaque
  references the chain only records, doesn't resolve.
- The platform chain
  ([ADR-010 §E](ADR-010-platform-operator-access.md)) preserves
  staff-action rows against now-erased tenants without modification.

Hash-evidence is unchanged because no row is altered. *Resolvability* of
those references is reduced — exactly the GDPR effect
([ADR-004 §D](ADR-004-defence-in-depth.md) GDPR-reconciliation).

### G. Self-host — the question collapses

A self-host install is a single tenant
([ADR-010 §H](ADR-010-platform-operator-access.md)); no second controller,
no maintenance grant, no other audit chain. The DEK rules in A still apply
(user-owned vs tenant-owned), but the cross-tenant edge cases don't arise.
Federation across multiple linked installs is a future ADR's question.

## Consequences

**Positive.** B keeps B's regulatory record — Part-145 retention isn't at
the mercy of A's erasure. Erasure stays effective — A's content gone, B's
references resolve to "deleted." No DEK crosses the controller boundary —
the structural property that makes crypto-shred work is preserved exactly.
Mirrors converge — `resource.reference-erased` lets consumers reconcile
explicitly. Audit integrity untouched. Rule is one line: *controller's
DEK; reference others by opaque ID*.

**Negative.** Resolver handles three states everywhere — `present`,
`soft-deleted` (tombstone), `controller-erased` (dangling); every
serialiser dealing with cross-controller references handles all three. A
new event class joins the catalog; consumers not subscribed to it will
see a stable reference silently stop resolving. Pseudonymous references
look odd in BI — a report counting maintenance hours by operator shows
"former tenant" buckets. Competency-evidence DEK placement is a schema
invariant — getting it wrong loses evidence when a pilot leaves the
tenant; mitigated by typing the column and a CI lint.

**Neutral.** `KeyProvider::for_record(controller)` is mechanical, easy to
test. The dangling-reference shape is a small envelope of metadata
(`{ id, status, deleted_at, deletion_reason }`) — not new PII, just the
negative space. Erasure of a user with tenant signoffs on their
(shredded) logbook collapses cleanly: log entry's encrypted content is
gone with the user DEK; the signoff was *in* the pilot's logbook, not in
B's records — nothing to reconcile on B's side.

## Alternatives considered

- **Shared / cross-tenant DEKs for shared records.** Breaks crypto-shred
  (erasing one tenant can't shred a key the other still holds); creates a
  covert escape from the per-controller boundary
  ([ADR-001 §D](ADR-001-platform.md)).
- **Cross-controller reference encrypted under the other tenant's DEK.**
  A's erasure makes B's row uninterpretable — silently corrupts B's
  compliance record. References must remain plaintext-opaque so the
  *record* survives the *referent*.
- **Copy referenced data into B's boundary.** Materialises a partial
  mirror of A inside B; updates don't propagate; GDPR analysis becomes
  complex. A small minimisation snapshot of the few fields B *needs* for
  its record (tail number string, type designator) is acceptable per
  record and is already accommodated by A's rule.
- **Propagate erasure into other controllers' records.** Corrupts B's
  regulatory retention (B must keep authored history under §145.A.55) and
  breaks the no-row-alteration audit invariant. Pseudonymisation by
  dangling is the right shape.
- **Emit `resource.deleted` for the dangling-reference case.** Confuses
  consumers — `maintenance_record.deleted` reads as "B's record is
  deleted," but B's record is *alive* and A's reference *in* it is gone.
  A distinct event class is cheap and honest.
- **Defer to the first real Part-145 integration.** Schema invariants are
  expensive to retrofit ([ADR-003](ADR-003-db-migrations.md) forward-only);
  an implementer without a rule reaches for the wrong default. Writing it
  now costs little; writing it wrong later costs a backfill.

## References

- [ADR-001 §D/§E/§G](ADR-001-platform.md) — envelope encryption + crypto-shred (the model this ADR completes); sub-processor (B's webhooks about A's erasure); safety key stays separate.
- [ADR-003 §B](ADR-003-db-migrations.md) — expand-contract on the DEK assignment column; getting it wrong costs a backfill.
- [ADR-004 §D](ADR-004-defence-in-depth.md) — no-PII audit; opaque UUIDs mean erasure doesn't require row modification.
- [ADR-005 §C](ADR-005-workspace-layout.md) — `KeyProvider::for_record(controller)` resolver in `flight-academy-store`.
- [ADR-006 §D](ADR-006-api-contract.md) — opaque prefixed IDs as the reference shape; dangling-reference resolver shape.
- [ADR-007 §D](ADR-007-sync-filtering-deletion.md) — delete-vs-erase (this ADR completes the cross-tenant case); `resource.reference-erased` joins the catalog alongside `resource.deleted` / `resource.erased`.
- [ADR-008 §B](ADR-008-data-sharing-posture.md) — data-class table holds; the dangling shape carries no PII.
- [ADR-009 §C/§D](ADR-009-event-streams-and-retention.md) — audit chains unaltered by erasure; references in historic rows stop resolving.
- [ADR-010 §E](ADR-010-platform-operator-access.md) — platform-chain rows referencing erased tenants preserved unchanged.
- [ADR-011](ADR-011-user-consent-grant.md) — user-owned (E1/E2) uses the per-user DEK; user-erasure shreds it, grants resolve to revoked.
- [domain-model §2.3 / §3 / §4](../design/domain-model.md) — maintenance-grant and cross-plane records; the cross-tenant relationship; sensitivity tiers.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (truth: erasure honestly signalled); 35–36 (restraint: one-line rule); 38 (diligence: invariant locked now); 48 (watchfulness: B's compliance protected).
- EASA Part-145 §145.A.55; UK CAA Reg 1321/2014; GDPR Art. 17 + Art. 11.

## Notes

Load-bearing decision is **A**: the controller-owner rule. Once in place,
every other question — references, events, audit, user-owned data crossing
an erased tenant — answers itself by application of the rule plus
[ADR-007](ADR-007-sync-filtering-deletion.md)'s delete-vs-erase. The
complexity that *appears* in this ADR is mostly *explanation* of the
consequences; the decision is one line.

`resource.reference-erased` is small but important — the difference
between a consumer ecosystem that silently drifts and one that explicitly
reconciles. Naming it now means the catalog and SDKs are correct from day
one.
