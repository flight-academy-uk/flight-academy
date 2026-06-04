# ADR-010 — Platform-operator / staff cross-tenant access

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | platform, staff, abac, break-glass, audit, privacy, hosted-only |
| **Supersedes** | (none — refines [ADR-001 §C](ADR-001-platform.md), see B) |

## Context

The hosted offering needs **staff** — operators of the platform — to act
across tenants in limited audited ways: support recovering a stuck booking;
CS investigating a churn signal; SRE on-call inspecting a queue;
maintainer rolling out a deployment. domain-model §1.3 names this third
authorisation plane.

Earlier ADRs deferred this on purpose:

- [ADR-001 §C](ADR-001-platform.md) — ABAC `Subject` carries a single
  `tenant_id`; no model for cross-tenant staff.
- [ADR-004 §D](ADR-004-defence-in-depth.md) — anticipated the audit slot
  (`actor_type IN ('user','system','api','admin')`) and the deception layer
  (§E) without saying who `admin` is.
- [ADR-008 §A](ADR-008-data-sharing-posture.md) — confirmed staff plane is
  **out of the product OpenAPI contract**.
- [ADR-009 §C](ADR-009-event-streams-and-retention.md) — reserved a
  **platform chain** for staff actions.

This ADR fills the joint decision: who elevates, against what tenant, for
what reason, for how long, with what authentication strength, what they may
do, how the deception layer recognises misuse.

Constraints: trust asymmetry — staff is the single highest-blast-radius
identity, so controls must be substantially stronger and the surface
smaller; GDPR — staff seeing personal data needs a documented operational
reason per access; CAA/EASA auditors look for evidence access was bounded,
recorded, reviewed (the platform chain is the regulator-readable record);
self-host ([ADR-002 §A](ADR-002-release-deployment.md)) — a self-hoster is
their own operator and the break-glass machinery is hosted-only;
[CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (no shadow access), 48
(watchfulness on the highest-risk identity), 35–36 (smallest staff surface).

## Decision

**The staff plane lives outside the product API on its own authenticated
surface. ABAC is extended with `Subject.actor_class = Staff` whose policy
requires a *just-in-time, time-boxed, justified break-glass elevation*
against a specific tenant. Staff actions log to the platform chain carrying
role, elevation id, and ticket reference. Special-category data is
absolutely forbidden. On self-host the whole plane is disabled.**

### A. Plane separation — out of the product API

The staff plane is **not** part of the public OpenAPI surface
([ADR-008 §A](ADR-008-data-sharing-posture.md)). A small internal admin web
app on a separate origin (`ops.flight-academy.app` or VPN-only) with its
own certificate and WAF profile, behind an SSO/IdP gateway (D), reachable
only over WireGuard/Tailscale or an IP-allow-listed corporate ingress — never
the open internet. Own thin handler set, no generated SDKs, no public spec.

It shares the codebase, the auth crate, and the audit infrastructure with
the product, but is a distinct deployable: handlers behind a feature flag +
build-time check that the deployment is `mode=hosted`. A `mode=self-host`
build does not link the staff plane at all (H).

### B. Subject extended with `actor_class` (refines ADR-001 §C)

```rust
// Illustrative; final API may differ.
pub enum ActorClass { Member, Staff, System }

pub struct Subject {
    pub user_id:     Uuid,
    pub actor_class: ActorClass,
    pub tenant_id:   Option<Uuid>,        // None for Staff until elevated
    pub roles:       BTreeSet<Role>,      // membership roles or staff roles
    pub attributes:  SubjectAttributes,
    pub elevation:   Option<Elevation>,   // staff break-glass grant (C)
}
```

`tenant_id` becomes `Option<Uuid>` because Staff isn't bound to a tenant by
default; it acquires tenant context only through an elevation (C). Every
product-API call still requires a `tenant_id`
([ADR-006 §C](ADR-006-api-contract.md)) — **Staff cannot call the product
API at all**; they call only the staff-plane admin handlers.

The Accepted §C text is read through this paragraph (same reconciliation
pattern as ADR-005's `fa-*` table).

### C. Break-glass — just-in-time, time-boxed, justified

| Field | Notes |
| --- | --- |
| `elevation_id` | UUID; appears in every audit row of the session |
| `staff_user_id` | who elevated |
| `tenant_id` | the specific tenant accessed |
| `purpose` | enum: `support` / `cs` / `incident` / `audit-prep` / `deployment` |
| `justification` | short free text linked to a ticket (mandatory) |
| `ticket_ref` | external reference (`SUPPORT-1842`); required for purposes that touch member data |
| `granted_at`, `expires_at` | bounded session — default **30 min**, max **2 h** |
| `granted_by` | co-signer; required for cross-tenant or no-ticket purposes |
| `revoked_at`, `revoked_reason` | early termination |

**No standing all-tenant access.** Per-incident, per-tenant. Three tenants
to investigate = three elevations, each with its own justification and
ticket. Session-extend requires a fresh elevation; the new row records why.

### D. Authentication — stronger than members

Staff authenticate through the **corporate IdP (OIDC, passwordless)**, then
to the staff plane:

- **Hardware-backed passkey only** — no magic-link, no software passkey, no
  push. Must attest a hardware authenticator (FIDO2 attestation chain).
- **Short re-auth** — staff session expires after 8 h inactive / 24 h
  absolute; elevations (C) are independent and shorter.
- **No staff API tokens in v1.** Scripted incident-response signs in
  interactively each time. A `staff:script` model gets its own ADR if
  needed.
- **IP-bound sessions** to the corporate network range.

### E. Audit — every staff action on the platform chain

Every Staff action — even a read — writes to `audit_events` on the platform
chain ([ADR-009 §C](ADR-009-event-streams-and-retention.md)) carrying:

- `actor_type = 'admin'`, `actor_id = staff_user_id`
- `staff_role`, `elevation_id`, `purpose`, `ticket_ref`
- `tenant_id` (never NULL for staff actions)
- standard fields (`action`, `resource_type`, `resource_id`,
  `decision_rationale`, `source_ip`, `user_agent`, `prev_hash`, `row_hash`)

Independently verifiable; archived per
[ADR-009 §D](ADR-009-event-streams-and-retention.md). Quarterly review is
operational discipline. "Who accessed this tenant in the last 90 days?" is
a one-query answer.

### F. Honeypots and canaries — staff-specific

Extends [ADR-004 §E](ADR-004-defence-in-depth.md):

- **Honeypot tenants** — synthetic tenants no legitimate staff member
  should elevate against. Any elevation is high-signal — no false-positive
  path.
- **Honeypot member records inside real tenants** — synthetic members
  inside legitimate tenants; any staff read triggers an alert. Catches an
  attacker who has staff credentials but doesn't know the traps.
- **Canary credentials** — staff sessions with bot patterns (sequential
  reads across many tenants, abnormal time-of-day, fixed interval) raise an
  anomaly signal.

Specific identifiers and thresholds live in
`docs/operations/hardening.md` (private; consistent with ADR-004's
public/private split).

### G. Scope — what staff can do

- **Reads** of operational + personal (tenant-context) data, scoped to the
  elevation's tenant.
- **Writes** are a small named set: `member.unlock`,
  `member.reset_passkey_enrolment`, `tenant.suspend`,
  `booking.cancel` (with the tenant's request), and similar discrete
  operations. Each write needs a per-action confirmation (second click,
  parameters re-stated).
- **Bulk reads / exports** are forbidden — tenants needing exports use the
  user/tenant-initiated path
  ([ADR-008 §E](ADR-008-data-sharing-posture.md)).
- **Special-category data is absolutely forbidden.** Staff cannot see
  medical detail, safety-reporter identity behind the safety key
  ([ADR-001 §G](ADR-001-platform.md)), or any class-special field
  ([ADR-008 §B](ADR-008-data-sharing-posture.md)). The data-class rule
  applies at the field level regardless of role; the staff plane has *no*
  escalation that crosses the never-leaves boundary.
- **User-owned personal data** (logbook, competency, medical references) is
  visible only when the data subject's tenant is the elevation's tenant; the
  data subject is informed where regulation or contract requires it. The set
  of cases is small and named per purpose; default is no.

### H. Self-host — staff plane disabled entirely

A `mode=self-host` build **excludes** the staff-plane handlers, the admin
web binary, the elevation table, and the platform chain. **Self-host has no
staff plane and therefore no corporate IdP, no hardware-passkey enrolment,
no break-glass machinery to operate.** The self-hoster is their own
operator within a single tenant; cross-tenant staff access is undefined
and cannot exist by construction. The `actor_class` enum still carries
`Staff` (for code consistency) but no policy admits it.

This removes the platform plane's entire operational and procurement burden
from self-hosters, and removes a class of "self-hoster reads their own users'
medicals through an admin panel" risk that would otherwise need its own
posture decision.

### I. Runtime topology — separate binary, internal spec

Refines §A. The staff plane runs as its own binary (`apps/admin`, see
[ADR-005 §F](ADR-005-workspace-layout.md)), not a feature-flagged variant
of `apps/api`. Code separation is the compile-time guarantee: tenant
routes cannot load staff endpoints, and the elevated PG role
(**`app_platform`** — added to the role list in
[ADR-002 §F](ADR-002-release-deployment.md) /
[domain-model §7.8](../design/domain-model.md#78-db-roles-per-adr-002-f--adr-003);
sets `app.actor_class = 'platform'` per E) and
`platform:*` ABAC scopes are unreachable from the tenant binary by
construction. The Kubernetes Service is `ClusterIP`, never
`LoadBalancer`; network policies deny tenant-pod ingress; the binary
refuses to bind a publicly-routable interface without an explicit
opt-in env-var, audited at startup.

**OpenAPI surface.** The staff spec is served only by the staff binary
at `/openapi.yaml` and `/docs`, behind staff auth. **Not** committed to
the public repo, **not** published to a portal, **not** consumed by
public client generators. This is reconnaissance friction, not access
control — the spec is reproducible from the public AGPL source. The
internal-only stance reduces opportunistic attacker convenience and
spec-driven tooling reach; it does not substitute for correct
authentication, authorisation, network isolation, or audit.

**Scope.** Application-authorised, tenant-aware operations:

- Break-glass elevation (G).
- Tenant lifecycle, branding, feature-flag overrides.
- Audit-chain integrity, DEK lifecycle, crypto-shred status
  ([ADR-012](ADR-012-cross-tenant-dek-erasure.md)).
- Outbox lag, webhook delivery rates, cert expiry — *per tenant*.
- Tenant-aware progressive delivery (canary promotion gated by
  tenant-specific error budget).
- Compliance — pending erasures, DSAR workflow, breach notification.

**Out of scope.** Cluster-shape observability (CPU, memory, request
rate, traces) and generic cluster admin (Argo UI, Flux UI without
tenant context) belong on the internal observability and cluster-admin
stacks. The staff binary **deep-links** into those — URLs carrying the
current tenant/time context — rather than embedding them. Their
availability does not depend on the staff plane being up.

### J. Tenant transparency reporting

Platform-staff actions against a tenant are reported back to that
tenant. Three channels read from the same source — platform-chain
entries (E) scoped by `tenant_id`:

- **Real-time** for high-sensitivity actions (data export, DEK access,
  user-record reads, writes on tenant data). Email + in-app at the
  moment of access. Carries: actor role, pseudonymous actor id,
  timestamps, ticket reference, structured reason, resources touched,
  dispute link.
- **Periodic digest**, default monthly, tenant-configurable. Summarises
  all platform-staff actions in the window — sent even when empty, so
  silence is itself a signal.
- **On-demand pull** at `GET /me/tenant/platform-actions` returning
  tenant-relevant platform-chain entries. Supports DSAR fulfilment.

Audit entries (E) are unchanged and non-optional; dispatchers are
downstream consumers.

**Delayed disclosure.** Entries carry `release_at`, defaulting to *now*.
Legal holds — non-disclosure orders, active incidents pre-patch, fraud
investigations — set `release_at` forward under a documented ceremony,
itself a platform-chain entry. The audit row is created regardless;
only the tenant-facing notification is deferred. Holds lift
automatically at `release_at`; manual release is also audited.

**Actor identity.** Real role + pseudonymous individual id. The
pseudonym-to-identity mapping lives only in the platform chain and is
queryable only via formal process. Tenants cite the pseudonym in
disputes without inducing individual harassment.

**Honeypot tenants.** Dispatchers no-op silently (F); the audit entry
still records — a notification to a sentinel would itself signal the
actor that they tripped a honeypot.

**Self-host.** Not applicable — no platform staff distinct from the
tenant (H). Transparency within the self-hosting organisation is its
deployment policy.

**Schema.** Platform-chain rows carry `human_reason` and
`tenant_visible` (default `true` for tenant-scoped actions) and
`release_at` (default `now()`). Real-time-eligible action types are
enumerated and stable. See
[domain-model §7.4](../design/domain-model.md#74-audit--event-stores-per-adr-009).

## Consequences

**Positive.** Bounded trust surface — no standing all-tenant access; the
blast radius of a compromised staff credential is one elevation window.
Regulator-defensible — one query answers "who accessed what, why, when,
how long." Stronger auth where the risk is highest (hardware passkey +
short session + IP-bound + co-signature for cross-tenant purposes).
Special-category data is unreachable. Self-host gets simpler — no staff
plane shipped at all.

**Negative.** Operational friction — every action needs elevation +
justification + ticket; co-signatures slow cross-tenant analysis. Hardware
passkey is a procurement burden (one primary + one backup per staff
member). No staff API tokens means 50 member-unlocks = 50 authenticated
clicks. Ticket-system dependency (run our own or take a sub-processor with
its own DPA). Admin web app is a separate deployable.

**Neutral.** Subject extension (B) is a small mechanical change. The 30 min
/ 2 h defaults are operational tuning. Honeypot tenants need maintenance
(drifting synthetic data). The two-class IdP (members passwordless, staff
corporate OIDC + hardware) increases the surface a contributor must
understand — but the separation is the security property we want.

## Alternatives considered

- **Standing all-tenant access for senior staff.** Single compromised
  credential = total platform compromise; cannot be defended to a regulator
  asking "what bounded this?"
- **Per-tenant explicit grants only (no self-elevation).** Strongest
  privacy but urgent incident response can't wait for a tenant admin —
  often the one who asked for help. Kept as an option for low-priority
  purposes (audit prep, CS investigation outside an active ticket) with
  `granted_by` co-sign + tenant acknowledgement.
- **Customer-impersonation cookie / token.** Confuses audit attribution
  (looks like the member acted), unclear GDPR basis, breaks if the user
  signs in concurrently.
- **Read-only PG superuser for staff.** Bypasses ABAC, RLS, audit, the
  data-class rule, the field-level sensitivity check. Staff with DB access
  is staff with no access controls.
- **Fold into product ABAC with a `platform_admin` role.** ADR-001 §C's
  single-valued `tenant_id` is by design; cross-tenant inside the same
  model destroys "one tenant per request" (clean RLS, simple cache).
- **Allow special-category access under elevated purposes** (legal-hold,
  safety-investigation). Rejected — the never-leaves rule
  ([ADR-008 §B](ADR-008-data-sharing-posture.md)) holds against staff too.
  Regulator-driven special-category access is satisfied by the tenant's
  own safety-officer or DPO.

## References

- [ADR-001 §C/§F/§G/§H](ADR-001-platform.md) — ABAC `Subject` extended (B); sessions; safety key (protected from staff in G); no-telemetry.
- [ADR-002 §H](ADR-002-release-deployment.md) — `mode=hosted` vs `mode=self-host` build toggle (A, H).
- [ADR-004 §C/§D/§E](ADR-004-defence-in-depth.md) — constant-time auth; `actor_type='admin'` filled by E; deception layer extended in F.
- [ADR-006 §C](ADR-006-api-contract.md) — product API path model unchanged; staff plane is separate, not `/admin/*`.
- [ADR-008 §A/§B/§E](ADR-008-data-sharing-posture.md) — staff out of product API (A); data classes hold against staff per G; export paths remain user/tenant-initiated.
- [ADR-009 §C/§D](ADR-009-event-streams-and-retention.md) — platform chain populated by E; archival applies.
- [domain-model §1.3 / §6.4](../design/domain-model.md) — names this plane; references this ADR.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (no shadow); 35–36 (smallest surface); 48 (watchfulness on the highest-risk identity).
- [SECURITY.md](../../SECURITY.md) — operator-side compromise threat model.
- FIDO2/WebAuthn 3 — <https://www.w3.org/TR/webauthn-3/>; OIDC Core 1.0 — <https://openid.net/specs/openid-connect-core-1_0.html>; NIST SP 800-63B — <https://pages.nist.gov/800-63-3/sp800-63b.html>.

## Notes

Load-bearing decision is C: elevation as a time-boxed, justified,
per-incident grant rather than standing access. Everything else is in
service of making C effective and recordable. Test for any future
"convenience" standing role: *what does this look like to a regulator
auditing access to a tenant's data?* If the answer cannot point at a
platform-chain row naming a person, a tenant, a time window, and a ticket,
the convenience isn't worth what it gives up.

Second load-bearing decision is H: a self-hoster is their own operator and
shipping a staff plane to them would be a privacy hazard and a maintenance
burden the AGPL ethos does not require. The cleanest answer is not to ship
that surface at all.
