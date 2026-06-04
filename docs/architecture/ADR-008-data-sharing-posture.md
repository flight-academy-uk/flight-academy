# ADR-008 — API data-sharing posture and scope model

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | api, privacy, gdpr, scopes, data-sharing, authorisation, posture |
| **Supersedes** | (none) |

## Context

Flight Academy holds regulated and often special-category personal data
(medical detail, training records, safety-reporter identity, financials).
The platform's whole stance is that the operator and the data subject are
**in control** — [ADR-001 §D](ADR-001-platform.md) makes erasure mathematically
effective via crypto-shred; [ADR-004 §D](ADR-004-defence-in-depth.md) audits
privileged access; [ADR-001 §H](ADR-001-platform.md) commits to no telemetry.

[ADR-006](ADR-006-api-contract.md) + [ADR-007](ADR-007-sync-filtering-deletion.md)
were initially framed around third parties *mirroring* tenant data — the
Stripe-style "replicate everything" the portal mockup assumed. That premise
overshoots: bulk replication escapes our erasure, audit, and control
guarantees. Once a member's medical or a tenant's safety reports are copied
into N third-party systems, Article 17 becomes "please ask N vendors to
delete," which is contractual and unenforceable.

Four data flows, only one of which is a privacy problem:

1. **Scoped action / integration** — external system reads or writes
   specific records for a workflow (school's booking widget; HR reading
   instructor roster + hours).
2. **Event-driven side effect** — external system reacts to a webhook, no
   bulk store (`booking.created` → Slack).
3. **Bulk mirror / replication** — external system keeps a full synchronised
   copy (data warehouse, Airbyte/Fivetran).
4. **Portability / export** — user or tenant takes *their own* data out
   (Art. 20).

Almost every real integration is (1) or (2). (3) is mostly a hosted-only
concern (self-hosters have the database). (4) is consented and
subject-initiated. First-party accounting integrations
([ADR-001 §E](ADR-001-platform.md)) already use the "we orchestrate" pattern;
that is the model to generalise.

**Ownership split** ([ADR-001](ADR-001-platform.md)): tenant-owned operational
data vs user-owned personal data (logbook, medical, ratings, competency).
A tenant is the controller of the former and **never** of the latter; a
tenant credential bulk-exporting a member's medical is a category error.

Constraints: GDPR minimisation + erasure reach; AGPL self-host (the API is
not the only way to obtain data); [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md)
instruments 28 (don't claim privacy-by-default while shipping a firehose),
35–36 (no capability beyond need), 48 (what does a bulk-export look like to a
careless integrator?).

## Decision

**The public API is integration-first and minimisation-first: scoped,
consented access for specific workflows and event-driven side effects is
first-class; bulk synchronisation is a deliberately-gated, opt-in, audited
capability over a tenant's *own operational* data only; special-category
data never leaves the boundary through the API at all; user-owned personal
data is reachable only with the owning user's consent.**

### A. Posture — integration-first, not mirror-first

The API exists so tenants and users can do specific things and react to
events, not so third parties can replicate datasets. First-class capability
is flows 1–2. Flow 3 is opt-in and gated (D). Flow 4 is a separate
subject-initiated export (E). **Standing stance**, like
[ADR-001 §H](ADR-001-platform.md) no-telemetry: contribution/design gate, not
a default to relax endpoint by endpoint.

### B. Data classes — what may leave

Every API-exposed field falls into one of four classes (tied to
[domain-model §4](../design/domain-model.md)):

| Class | Examples | May leave via the API? |
| --- | --- | --- |
| Operational, non-personal | fleet, schedule slots, aircraft status | Yes, per scope |
| Personal, tenant-context | member roster, employment status, hours taught | Minimised projections only, per scope + ABAC |
| User-owned personal | logbook, competency, medical references | Only via the **owning user's** credential/consent — never a tenant key |
| Special-category / protected | medical-certificate detail, safety-reporter identity | **Never** — no scope can exfiltrate it |

Safety-reporter identity stays behind the separate safety key
([ADR-001 §G](ADR-001-platform.md)); it has no API representation. The
"never leaves" row is absolute — even an elevated bulk scope cannot reach it.

**Class lives at the *field* level, not the resource level.** A single
resource typically carries fields from more than one class — `safety:read`
returns occurrence operational fields without `reporter_id`. A serialiser
that emits a field whose class exceeds the caller's scope is a bug.

### C. Scope model — least privilege, sensitivity-graded

Scopes name `resource:action` (`bookings:read`, `instructors:read`,
`webhooks:manage`), extending [ADR-006 §G](ADR-006-api-contract.md).

- **Least privilege.** Default issuable scopes are narrow read/write on
  *operational* resources.
- **Sensitivity-graded.** Reading personal data needs a distinct elevated
  scope class (e.g. `…:personal`), visually separated when a tenant admin
  issues a key.
- **Bulk/sync is its own elevated class** (`…:sync`), off by default (D).
- **Scopes gate the key; ABAC still evaluates per request** — a scope can
  only further restrict ([ADR-001 §C](ADR-001-platform.md)).
- **User-owned data scopes ride only user-authorised credentials** — the
  user's own session/token or an OAuth grant the user consented to, never a
  tenant-admin-issued key (specified by
  [ADR-011](ADR-011-user-consent-grant.md)).

### D. Bulk synchronisation — gated, minimised, audited

The `updated_since` feed and bulk reads
([ADR-007 §B](ADR-007-sync-filtering-deletion.md)) are available to a
*third-party* key only when **all** hold:

- tenant admin has explicitly enabled bulk/sync access and granted the
  elevated scope (C);
- the data is the tenant's **own operational** data — not user-owned, not
  special-category (B);
- every bulk read is audited
  ([ADR-004 §D](ADR-004-defence-in-depth.md)) with actor, scope, resource,
  volume;
- bulk-read scopes carry a distinct rate-and-volume budget separate from
  the per-key default ([ADR-006 §G](ADR-006-api-contract.md)) — a sync key
  should not drown out interactive use;
- the tenant can see and revoke which keys hold sync access.

Bulk sync is **off by default**. Our own first-party clients use the same
`updated_since` primitive under first-party credentials (mobile offline-sync
depends on it), so ADR-007's machinery is justified regardless of how
tightly third-party access is gated.

### E. Portability and erasure — first-party, consented paths

- **Export (Art. 20)** is user/tenant-initiated, authenticated, audited
  (`me/privacy`; domain-model §2.1/§2.12) — a deliberate act, not a
  continuous third-party pull.
- **Erasure propagation.** Where data has legitimately left through a
  granted integration, propagation is contractual via the sub-processor /
  DPA terms ([ADR-007 §D](ADR-007-sync-filtering-deletion.md);
  [ADR-001 §E](ADR-001-platform.md)). Because special-category and user-owned
  data never leave except by the owner's hand, the set of external places
  erasure must reach is deliberately small.

### F. Consent and transparency

Issuing a key or authorising an integration shows exactly which data
classes and scopes it grants; sensitivity grades are visually distinct;
nothing is granted implicitly (Trust-Centre posture, domain-model §2.12).

## Consequences

**Positive.** Erasure stays effective — minimised data leaves, special-
category never. Real integrations (HR, calendar, Slack, widgets, accounting
via first-party adapters) are fully supported as flows 1–2. Ownership is
enforced, not aspirational. Privacy posture is a differentiator, not a
limitation. Bulk sync exists for the legitimate "tenant's own warehouse"
case without being the default invitation.

**Negative.** Less frictionless for a would-be full mirror (deliberate).
More design and review surface — the data-class matrix must be maintained
as resources are added. Some integrations are simply not buildable on our
API (e.g. third-party medical-records pull); accepted — the cost falls on
the integrator. User-owned access needs the OAuth-style consent flow in
[ADR-011](ADR-011-user-consent-grant.md).

**Neutral.** Portal framing shifts from "mirror our data" to "integrate with
us" — the mockup is aspirational on the mirror point. Self-hosters are
unaffected (they own their DB). Aggregate / non-PII surfaces (school
directory, status page, CV-verify) were never personal-data mirrors.

## Alternatives considered

- **Mirror-first (Stripe-style).** Maximally extensible, lowest-friction
  for data-pipeline consumers. Maximises the surface where erasure cannot
  reach; turns "subject is in control" into a contractual fiction.
- **No third-party data API.** Most private but kills legitimate
  integrations (HR, calendar, widgets); hostile to AGPL/community use.
- **Events + scoped actions only, no bulk read.** Strong but forbids a
  tenant's own data warehouse, pushing tenants toward DB-level extraction
  that's harder to audit. We default to this and add **gated** bulk on top.
- **Special-category via an elevated scope.** Rejected outright — no scope
  exfiltrates the data whose protection is the just-culture cornerstone
  ([ADR-001 §G](ADR-001-platform.md)).
- **DPA contract terms instead of technical minimisation.** Contracts cover
  the residual; minimisation is the primary control (instrument 28).

## References

- [ADR-001 §C/§D/§E/§G/§H](ADR-001-platform.md) — ABAC; crypto-shred; first-party integrations + DPA; safety key; in-control stance.
- [ADR-004 §D](ADR-004-defence-in-depth.md) — audit of bulk reads and key use.
- [ADR-006 §G](ADR-006-api-contract.md) — scope surface this grades.
- [ADR-007 §B/§D](ADR-007-sync-filtering-deletion.md) — bulk-sync machinery this gates; erasure-vs-delete.
- [ADR-009](ADR-009-event-streams-and-retention.md) — reads §B's taxonomy to decide what's audited; bulk reads always audited.
- [ADR-010](ADR-010-platform-operator-access.md) — staff plane this posture says is out of the product API.
- [ADR-011](ADR-011-user-consent-grant.md) — OAuth 2.1 + PKCE implementing C's "user-authorised credentials only."
- [ADR-012](ADR-012-cross-tenant-dek-erasure.md) — controller-owner DEK rule preserves erasure reach across cross-tenant refs; honours B's no-PII in `resource.reference-erased`.
- [domain-model](../design/domain-model.md) — §2.1/§2.12 export & consent; §2.13 developer surface; §4 sensitivity tiers; §6.5 mockup-overshoot note.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (truth); 35–36 (restraint); 48 (watchfulness).
- GDPR Art. 5(1)(c) / 17 / 20 — <https://gdpr-info.eu/>.

## Notes

**The test.** Every future endpoint or scope passes through one question:
*does this let data escape our erasure/audit boundary beyond the minimum a
real workflow needs, and could it carry special-category or user-owned data
without the owner's consent?* If yes, it does not ship in that form.

**Future consideration — computed write-back / external processors.** A
partner may want to compute on our data and have a result land in our
records (payroll/tax engine deriving fees or VAT). We don't build this now,
but the stance is set: **we remain the system of record.** Preferred shape is
*we-orchestrate* — partner as a computation provider behind a
`flight-academy-integrations` adapter ([ADR-001 §E](ADR-001-platform.md)),
like `PaymentProvider`: we send minimal inputs, they return the calculation,
*we* validate and write. If a genuine partner-originated write-back is ever
required, it enters as a **validated proposal, never a raw authoritative
write** — own elevated `…:propose` scope below `…:write`; server-side
validation invariants; idempotent + concurrency-safe
([ADR-006 §E](ADR-006-api-contract.md)); audited with the partner key as
actor; partner is a GDPR Art. 28 processor under a DPA. Graduates to its
own ADR when a concrete need arises.

**Future consideration — privacy-preserving compliance evidence.** Some
integrations will want to *verify* a claim (medical valid; currency holds)
without seeing the data. On-ethos answer: **verifiable credential /
selective disclosure** — W3C VC with SD-JWT or BBS+ signatures (general-
purpose ZK only for richer predicates). A signed assertion the verifier
checks without exposing the underlying record — a *principled exception*
to B's "special-category never leaves," because a proof *about* the data is
not the data. Our Pilot CV (domain-model §2.8) is the conceptual neighbour.
The hard parts are governance (regulator acceptance, trust root,
revocation/freshness, verifier adoption), not cryptography. Graduates to its
own ADR when a concrete need and a workable acceptance path both exist.
