# Domain model & API surface

| Field | Value |
| --- | --- |
| **Type** | Living design reference (not an ADR) |
| **Status** | Working draft |
| **Last updated** | 2026-06-04 |
| **Owner** | @ICreateThunder |

Working map of the Flight Academy domain: bounded contexts, resources and
operations per context, core entities, sensitivity tiers. It is **living**;
ADRs are immutable. This doc cites them rather than re-deciding. It was
reverse-engineered from the UI prototype (~42 JSX views; see *Sources*) and
reconciled against ADRs 1–4 — divergences are in §6.

---

## 1. Host topology & authorisation planes

Three planes plus public unauth surfaces. Each has a different identity
model and a different blast radius.

### 1.1 Personal / account plane — root domain

Host: `app.flight-academy.app`. The signed-in user, aggregated across every
org they belong to. Per ADR-001's tenant/user model, data here is
**owned by the user**, portable when they leave a school.

Personal dashboard (cross-org roll-up), logbook, competency, pilot CV,
medical references, encryption-key management, account settings & devices,
notifications, GDPR self-service (export / erase), connected apps.

The cross-org dashboard is a **user-scoped fan-out** over the user's own
memberships — not a cross-tenant query (ADR-001 §A prohibits those).

### 1.2 Tenant plane — tenant subdomain

Host: `<slug>.flight-academy.app`. Per-tenant **ABAC** (ADR-001 §C);
tenant context resolves from the subdomain and is validated against the
JWT `tenant` claim (ADR-001 §F). Data is **owned by the tenant**.

Tenant home, fleet, scheduling, maintenance, ground school, safety/SMS,
accounting, analytics, membership, ops inbox, examiner workflow, community,
document signing, developer console, compliance admin.

Tenant **type** reshapes these surfaces: ATO/flight school, Part-145
maintenance org, airfield operator. An aircraft operated by one tenant may
be maintained by another (§3, cross-tenant maintenance).

### 1.3 Platform-operator plane — internal cross-tenant tools

Host: internal only (`fa-root`, dark UI). Flight Academy **staff** acting
across tenants: support, CS, SRE/status, deployments.

**Out of the product OpenAPI contract.** Specified in
[ADR-010](../architecture/ADR-010-platform-operator-access.md):
just-in-time time-boxed justified break-glass elevation; hardware passkey
with corporate IdP; audited as `actor_type='admin'` on the platform chain;
disabled on self-host.

### 1.4 Public surfaces — unauthenticated

Marketing/landing; auth flows; Trust Centre (`/trust`); Pilot-CV verify 
(`/v/{code}`); School Directory (`/schools`); public API docs
(`/developers`); status page (`status.flight-academy.app`).

---

## 2. Resource / operation map by bounded context

Conventions (formalised in [ADR-006](../architecture/ADR-006-api-contract.md)):
tenant resources `/api/v1/tenants/{tenant}/…`; personal `/api/v1/me/…`;
public reads `/api/v1/public/…`. Operations are representative.
**R** = read, **W** = write/state transition.

**Ownership.** `me/…` = user-owned and portable; `tenants/{tenant}/…` =
tenant-owned; `public/…` = public or capability-token. Cross-plane oddities
are tagged in their section: **`flight-log entry`** (user-owned with
tenant signoff, §2.5), **`competency`** (user-owned, evidence logged in
tenant, §2.7), **`maintenance-jobs`/`maintenance-records`** (authored by
the maintenance tenant against another tenant's aircraft via a grant,
§2.3). Sensitivity is a *field*-level attribute on top of these owners
(§4; [ADR-008 §B](../architecture/ADR-008-data-sharing-posture.md)).

### 2.1 Identity & Access — *personal plane + auth*

- `auth` — begin/verify magic-link, WebAuthn register/assert, push-approve;
  refresh, revoke (W). Org join-code resolve (R).
- `me` (profile, locale/units, pronouns) — R/W.
- `me/credentials` — passkeys, paired push devices, sessions: list, add,
  rename, revoke (R/W).
- `me/memberships` — list orgs + role(s); leave; join with code (R/W).
- `me/encryption-keys` — key inventory, recovery methods (R/W) — *see §4;
  v1 is server-side so this surface is reduced.*
- `me/privacy` — data-we-hold (R); export (Art. 20) and erase (Art. 17) (W).
- `me/connected-apps` — third-party OAuth grants the user authorised: list
  with scopes + client + last-used; revoke (R/W); per
  [ADR-011](../architecture/ADR-011-user-consent-grant.md).
- `me/developer-apps` — apps the user has registered as a developer
  ([ADR-011 §D](../architecture/ADR-011-user-consent-grant.md)).
- `invitations` (tenant-plane) — issue magic-link invites, CSV import (W).

### 2.2 Tenancy & Membership — *tenant plane*

- `tenants/{tenant}` — profile, white-label tokens, type, ICAO/airfield (R/W admin).
- `tenants/{tenant}/membership-tiers` (+ perks, eligibility, auto-promote) — R/W admin.
- `tenants/{tenant}/subscriptions` — member↔tier, cycle, status; member card/QR (R/W).
- `tenants/{tenant}/billing-config` — Stripe Billing, VAT, dunning (R/W admin).
- `tenants/{tenant}/partner-perks` — reciprocal airfields (R/W admin).

### 2.3 Fleet & Airworthiness — *tenant plane; cross-tenant maintenance*

- `tenants/{tenant}/aircraft` (+ `/{tail}`) — fleet list/detail, specs,
  status, add, status transition (R/W).
- `…/aircraft/{tail}/airworthiness` — ARC, insurance, checks, AD/SB
  timeline (R).
- `…/aircraft/{tail}/snags` — defects; log snag → links to maintenance (R/W).
- `…/aircraft/{tail}/documents` — POH/checklist/tech-log/W&B/etc., versioned;
  pinned sections; offline cache (R/W).
- `…/aircraft/{tail}/weight-balance` — profiles, envelope compute (R/W).
- `tenants/{operator-tenant}/maintenance-grants` — operator↔CAMO/Part-145
  grant; RLS exposes only granted tails to the maintenance tenant (R/W;
  operator admin issues, maintenance admin accepts).
- `tenants/{maint-tenant}/maintenance-jobs` — worklist, status
  (open|progress|parts|signoff|done), owner, ETA (R/W). *Cross-plane:
  authored by the maintenance tenant against an aircraft owned by the
  operator tenant.*
- `…/maintenance-records`, `…/parts` (parts inbox) (R/W).

### 2.4 Scheduling & Bookings — *tenant plane*

- `tenants/{tenant}/resources` — aircraft/sim/instructor as bookable, with
  ratings & availability (R).
- `tenants/{tenant}/bookings` — list/create/move/resize/cancel; conflict
  detection (R/W). **Booking validation gate**: medical valid, currency OK,
  weather, daylight, AOG.
- `me/availability`, `me/booking-preferences` (R/W).
- `tenants/{tenant}/schedule-runs` — auto-scheduler solver: run,
  proposals, apply/swap/reject (R/W async; see §5).

### 2.5 Flight Records & Training — *personal plane (with tenant signoff)*

- `me/flight-logs` (+ `/{id}`) — EASA-column logbook; create, edit,
  submit-to-instructor; export PDF/CSV (R/W).
- `me/currency` — day/night/IFR, 90-day landings, IPC (R).
- `me/medical` — class, expiry (R/W) — *sensitive, see §4*.
- `…/flight-logs/{id}/briefing`, `…/debrief` — aim/plan/fuel/IMSAFE;
  instructor notes + competency ratings + next steps (R/W).
- `…/flight-logs/{id}/track` — ADS-B/OGN + GPX/SkyDemon (R/W).
- `…/flight-logs/{id}/signoff` — instructor sign / first-solo
  authorisation (W; audited, e-sign).
- `me/preflight` — checklist; on complete stamps the booking and the log
  entry (W).

### 2.6 Learning / LMS — *tenant plane*

- `tenants/{tenant}/courses` (+ lessons, content, quizzes) — R/W.
- `…/mock-exams` — attempts, scoring, weak-area analysis (R/W).
- `…/exam-bookings` — date, location, examiner (R/W).
- `tenants/{tenant}/syllabus` — per-tenant CAA-approved (R).

### 2.7 Safety / SMS & Competency — *tenant + personal competency*

- `tenants/{tenant}/safety/occurrences` — TEM, severity, anonymity tier;
  state machine `draft→submitted→under_review→reported_to_caa→closed`
  (R/W). Reporter identity under the safety key; `safety_officer`-only
  de-anon, audited (ADR-001 §G).
- `…/safety/hazards` — hazard register, risk band (R/W).
- `…/safety/metrics` — reporting rate, trend (R).
- `me/competencies` (+ evidence log) — 9 EASA/ICAO competencies; portable,
  user-owned; instructor grades evidence (R/W).
- MOR export = ECCAIRS2 (ADR-001 §G).

### 2.8 Examiner & Documents — *tenant plane*

- `tenants/{tenant}/skills-tests` — item grading (SRG-1119); **sign &
  submit to CAA** (W; e-sign, audited).
- `tenants/{tenant}/signing-documents` — multi-party ordered counter-sign,
  passkey-witnessed, hash-chained, tamper-evident (R/W).
- `me/pilot-cv` + `public/cv/{code}` — verifiable credential, 90-day
  validity, QR, revocation (R).

### 2.9 Finance & Accounting — *tenant plane*

- `tenants/{tenant}/member-accounts` — balance, status, auto-pay (R).
- `…/transactions` — ledger (hire/lesson/landing/sim/membership/payment);
  refs INV/PMT; running balance (R/W). **Ledger is the source of truth for
  Xero sync.**
- `…/statements` — current balance, next DD, period summary; PDF (R/W).
- `…/rates`, `…/payment-methods` (R/W).
- `…/integrations` — connector status/config (R/W) — see §5.

### 2.10 Communications & Notifications — *cross-plane*

- `me/notifications` (+ prefs, quiet hours, DND) — list, mark read,
  settings (R/W). Categories mirror the event catalog (§5).
- `tenants/{tenant}/ops-threads` — multi-channel inbox, SLA, assignment,
  internal notes; AI-assisted replies + tool actions (R/W). *AI assist
  internal-only, not in the public API.*
- `tenants/{tenant}/community` — channels, messages, events/RSVP (R/W) —
  *the one plausible future client-side-E2E surface; v1 server-side, §4*.
- `tenants/{tenant}/announcements` (R/W).

### 2.11 Analytics & BI — *tenant plane (read models)*

- `tenants/{tenant}/analytics/*` — KPIs, cashflow, fleet utilisation,
  members funnel, safety trend, instructor stats, training throughput (R).
- `…/analytics/digest` — scheduled weekly digest: recipients, highlights
  (R/W).
- Aggregated, no PII leaves the tenant (ADR-001 §H spirit).

### 2.12 Compliance & Governance — *tenant admin + public read*

- `tenants/{tenant}/retention-rules` — data type → period → legal basis;
  cron run log (R/W).
- `tenants/{tenant}/subject-access-requests` — Art. 15/20/17 queue, due
  dates, exemptions (R/W). Erasure honours retention overrides via
  crypto-shred (§4).
- `tenants/{tenant}/breach-workflow` — 72h clock, drills (R/W).
- `tenants/{tenant}/audit-log` — read view over `audit_events` (R;
  RLS-scoped).
- `public/trust` — Trust Centre, sub-processors, badges, consent receipts (R).
- `tenants/{tenant}/caa-audit-pack` — checklist + export `.zip` (R/W).

### 2.13 Developer Platform — *public docs + tenant console*

- `tenants/{tenant}/api-keys` — name, scopes, prefix
  `fa_sk_{live,test,sandbox}_`; create, rotate, revoke (R/W).
- `tenants/{tenant}/webhooks` — endpoint, subscribed events, delivery
  health; HMAC-signed, retried (R/W).
- `tenants/{tenant}/oauth-clients` — registered third-party apps for the
  OAuth grant flow ([ADR-011](../architecture/ADR-011-user-consent-grant.md)).
- `public/openapi` — emitted spec + Postman; SDKs TS/Dart/Python.
- Status page small read API: components, incidents, subscribe (R).

**Posture**: integration-first, minimisation-first
([ADR-008](../architecture/ADR-008-data-sharing-posture.md)) — scoped
actions + events first-class; bulk sync gated over a tenant's own
operational data; special-category never leaves; user-owned syncs only
with user consent. **Not** a "mirror everything" API (§6.5).

---

## 3. Core entities & relationships

Most are conventional CRUD aggregates. The ones that **shape the contract
and the schema**:

- **User ⇄ Tenant is many-to-many via `membership`** (ADR-001), each with
  composite role(s) ("CFI · Tenant admin"). One account, portable.
- **`flight_log_entry` has dual ownership**: owned by the *user*
  (portable), carries *tenant context* + *instructor signoff*. Erasing a
  user must not destroy the ATO's statutory training record — resolved by
  the retention/crypto-shred split (§4) and the controller-owner DEK rule
  ([ADR-012](../architecture/ADR-012-cross-tenant-dek-erasure.md)).
- **Aircraft is cross-tenant**: operated by tenant A, maintained by tenant
  B (Part-145). `maintenance_job` and signoffs are authored by B against
  A's tail. Requires an explicit **`maintenance_grant`** (§2.3) with RLS
  exposing to B only the granted tails.
- **`resource`** unifies aircraft / sim / instructor for the scheduler;
  instructor ratings (FI/CRI/CFI) are bookable *attributes* (ABAC).
- **`events_outbox`** (ADR-003 §D) is the spine for anything crossing a
  system boundary; feeds notifications (§2.10) and webhooks (§2.13).
- **`audit_events`** (ADR-004 §D + ADR-009) references actors/resources
  by opaque UUID only; never PII.

Aggregate roots (grouped; see per-context map for fields):

> **Identity:** user, credential, session, membership, invitation,
> encryption_key. **Tenancy:** tenant, membership_tier, perk, subscription,
> billing_config. **Fleet:** aircraft, airworthiness_item, snag,
> aircraft_document, wb_profile, maintenance_job, maintenance_record,
> part, maintenance_grant. **Scheduling:** resource, booking, availability,
> schedule_run, schedule_proposal. **Records/Training:** flight_log_entry,
> currency (derived), medical, briefing, debrief, track, signoff,
> preflight. **LMS:** course, lesson, mock_exam_attempt, exam_booking,
> syllabus. **Safety:** safety_occurrence, hazard, competency,
> competency_evidence. **Examiner/Docs:** skills_test, signing_document,
> pilot_cv. **Finance:** member_account, ledger_entry, statement, rate,
> payment_method, integration. **Comms:** notification, ops_thread,
> community_channel, community_message, announcement. **Compliance:**
> retention_rule, subject_access_request, consent_receipt, breach_record,
> sub_processor. **Platform:** api_key, oauth_client, oauth_grant,
> webhook_endpoint, webhook_event, elevation, audit_event.

---

## 4. Data-sensitivity tiers

Server-side envelope encryption (ADR-001 §D), **not** client-side
zero-knowledge E2E (§6.1).

- **Tier 0 — plaintext at the app layer** (CNPG disk-level still). Status
  enums, schedule slots, aircraft specs, tier/perk config.
- **Tier 1 — envelope-encrypted** (AES-256-GCM under a per-tenant *or*
  per-user DEK wrapped by a KMS KEK). Personal/regulated: medical detail,
  address, passport. User-owned data uses the **per-user** DEK;
  tenant-owned uses the **per-tenant** DEK. Cross-tenant rows use the
  *authoring controller's* DEK
  ([ADR-012 §A](../architecture/ADR-012-cross-tenant-dek-erasure.md)).
- **Tier 2 — safety-reporter identity** under a **separate per-tenant
  safety key**, decryptable only by `safety_officer`, audited (ADR-001 §G).
- **Tier 3 — client-side E2E — NOT in v1.** Community chat (MLS) is the
  candidate future surface. Until then no API resource is an opaque blob.

**Erasure** = crypto-shred the relevant DEK
([ADR-001 §D](../architecture/ADR-001-platform.md)). Statutory retention
(CAA Reg 22, Part-145.A.55, Part-M.A.305, EASA 376 §10y, HMRC 7y) is
honoured in minimised/pseudonymised form; audit chains stay intact because
rows reference shredded subjects by dangling UUID (ADR-004 §D).

---

## 5. Cross-cutting mechanisms (all already in the ADRs)

- **Auth** — passwordless (magic link / WebAuthn / push); short-lived
  Ed25519 JWT + opaque refresh (ADR-001 §F). Auth-event stream +
  backoff/lockout + constant-time responses (ADR-004 §C).
- **Authorisation** — ABAC trait + decision enum in `flight-academy-auth`
  (ADR-001 §C); `Subject.actor_class` extended for Staff (ADR-010).
  Audited per ADR-009 §B (sensitive Permits + every Deny + elevated
  actions).
- **Domain events** — `events_outbox`, committed in the same tx as the
  state change; LISTEN/NOTIFY + polling sweep; idempotent handlers
  (ADR-003 §D). One outbox feeds notifications and webhooks.
- **Incremental sync & deletions** — `updated_at` + soft-delete tombstones
  (no PII); `updated_since` feed; GDPR erasure as `resource.erased`
  (distinct from `resource.deleted`); safe-lag watermark for commit-order
  safety. See [ADR-007](../architecture/ADR-007-sync-filtering-deletion.md).
- **Cross-tenant DEK + erasure-by-reference** — controller-owner DEK rule;
  opaque cross-controller refs (no DEK crosses); dangling pseudonyms +
  `resource.reference-erased` event. See
  [ADR-012](../architecture/ADR-012-cross-tenant-dek-erasure.md).
- **Audit** — append-only `audit_events`, RLS-scoped, hash-chained
  (per-tenant + per-user + platform), INSERT-only `app_api`, monthly
  partitioned, hot → Parquet cold (ADR-004 §D + ADR-009). Scope:
  sensitive Permits + every Deny + every elevated action (ADR-009 §B).
  Outbox / auth-event / webhook-delivery retention sized in ADR-009 §D.
- **User-consent grant flow** — OAuth 2.1 + PKCE for third-party access to
  user-owned data; short JWT + opaque rotating refresh; per-grant
  revocation; app is a GDPR Art. 28 processor (ADR-011).
- **First-party hosting of user-supplied visual assets** — any visual
  asset that will render in a high-trust UI (OAuth consent screen,
  tenant white-label surface) is **uploaded and stored by us, never
  referenced by URL**. Pattern applied at OAuth client registration
  ([ADR-011 §D](../architecture/ADR-011-user-consent-grant.md)) and
  tenant branding ([ADR-014 §F](../architecture/ADR-014-frontend-architecture.md)).
  Closes SSRF ([ADR-017](../architecture/ADR-017-outbound-http-ssrf.md)),
  XSS at consent-time, post-registration logo-swap phishing, user-IP
  leakage to third parties on render, and third-party-host
  unavailability breakage. Gains: build-time transcoding (AVIF /
  WebP / SVG-as-is / raster fallback), content negotiation at serve,
  long-TTL CDN-friendly caching via hashed filenames, validation at
  upload (MIME, dimensions, content scan). Any new surface that
  ingests user-supplied visual assets adopts this pattern.
- **Integrations** — single `flight-academy-integrations` crate, adapter
  trait per category (ADR-001 §E): Accounting (Xero/QuickBooks); Payments
  (Stripe/GoCardless); Banking (TrueLayer/GoCardless BAD); Aviation data
  (CAA NOTAM/EUROCONTROL/Met Office). UI implies later additions:
  flight-planning (SkyDemon), tracking (OGN), comms (Mailchimp/Twilio/Slack).
- **Payments — three flows**: recurring memberships (Stripe Billing, VAT,
  dunning); pay-as-you-fly ledger (GoCardless DD, Xero two-way);
  one-off voucher/experience (3-D Secure, ABTOT).
- **Scheduler** — `schedule_run` is a constraint solver: consumes
  currency/medical/weather/daylight/AOG/examiner-locks/syllabus-need;
  emits proposals.
- **Offline-first (mobile only)** — sync cursors, client-generated ids,
  idempotent upserts, ETags (ADR-006).
- **Defence in depth** — WAF + cache + budgets + circuit breaker bound
  the edge bill; `tower_governor` in-process; honeypots/canaries; no PII
  in logs (ADR-004). Staff plane has staff-specific honeypots
  ([ADR-010 §F](../architecture/ADR-010-platform-operator-access.md)).
- **No telemetry / no phone-home** (ADR-001 §H).
- **Time zones** — `*_at` is a UTC instant on the wire; resources needing
  local time carry an IANA `time_zone` and a `*_local` companion;
  authoritative value is always UTC ([ADR-006 §D](../architecture/ADR-006-api-contract.md)).
- **Self-host parity** — hosted-only mechanisms have self-host equivalents
  the operator owes: AWS Budgets + circuit breaker (ADR-004 §A) →
  operator-owned spend monitor + maintenance toggle; S3 Object-Lock audit
  archive (ADR-004 §D + ADR-009 §D) → MinIO with object-lock;
  CloudFront + AWS WAF → reverse proxy with rate-limit module;
  fck-nat egress → operator NAT. The hosted AWS bias in 1–4 is
  intentional; self-host parity is documented per decision. The staff
  plane (ADR-010) is **disabled entirely** on self-host — no IdP, no
  break-glass, no platform chain.

---

## 6. Reconciliations & open questions

Where the design assets and Accepted ADRs disagreed, the ADRs win.

### 6.1 Encryption: server-side envelope, not client-side E2E *(resolved)*

The view-35 "E2E" screen describes client-side zero-knowledge encryption
(MLS chat, BIP-39/Shamir recovery, QM archival key, server cannot read).
That conflicts with ADR-001 §D (server-side envelope) and breaks §G
(safety de-anon), ADR-004 §D (audit writes about content), ADR-003 §B/§E
(server-side backfills over encrypted columns). **Decision: v1 is
server-side envelope per §D.** Full client-side E2E is a future ADR;
community chat (MLS) is the candidate. The screen is aspirational UI.

### 6.2 Public-API tenant addressing → ADR-006 *(resolved)*

Portal mockup showed `/v1/bookings` (tenant from the key); ADR-001 §A
mandates `/tenants/{tenant}/…`. **Keep `{tenant}` in the path even for
the public API** with the tenant-scoped key required to match.

### 6.3 Crate naming → ADR-005 *(resolved)*

ADR-002 §F and ADR-003 reference `crates/fa-db/…`, `fa-auth`, `fa-store`,
`fa-integrations`. Chosen layout is verbose `flight-academy-*` with the
binary at `apps/api`. ADR-005 states the layout and reconciles the
incidental `fa-*` paths.

### 6.4 Platform-operator plane → ADR-010 *(specified)*

The staff plane (§1.3) is not in the product OpenAPI contract and isn't
modelled by ADR-001 §C's single-tenant ABAC `Subject`.
[ADR-010](../architecture/ADR-010-platform-operator-access.md) specifies
it: separate internal surface; `Subject.actor_class=Staff`; just-in-time
time-boxed justified break-glass; hardware-passkey + corporate IdP;
platform-chain audit; disabled on self-host.

### 6.5 Public-API data-sharing posture → ADR-008 *(resolved)*

The developer-portal mockup assumed a Stripe-style "mirror everything"
public API. That overshoots the privacy ethos (ADR-001 §D crypto-shred
reach, §H in-control stance, GDPR minimisation).
[ADR-008](../architecture/ADR-008-data-sharing-posture.md) makes the API
integration-first and minimisation-first: scoped actions + events
first-class; bulk sync gated; special-category never leaves; user-owned
data only with the user's consent.

### 6.6 Standing design-system notes (from the UI)

Not contract decisions, but to honour when the web port happens: one
shared status vocabulary (`--status-ok|warn|blocked|offline`); one alert
pattern (icon + timeframe + action); GDPR export/erase as top-level
Settings entries; white-label stress-test against a worst-case tenant
palette.

---

## 7. Schema invariants — what every replicated table must include

This appendix collects the schema rules scattered across the ADRs so an
implementer touching the first migration has a single reference. Each
invariant is cited to its source ADR; this section restates, it does not
decide.

### 7.1 Every replicated resource (per [ADR-007](../architecture/ADR-007-sync-filtering-deletion.md))

| Invariant | Source |
| --- | --- |
| `updated_at TIMESTAMPTZ NOT NULL`, set by a `BEFORE UPDATE` trigger to `now()` on every row update | ADR-007 §B |
| `deleted_at TIMESTAMPTZ NULL` for soft-delete; bumping `deleted_at` also bumps `updated_at` | ADR-007 §C |
| `deletion_reason` enum/text column populated whenever `deleted_at` is set | ADR-007 §C |
| Default query filter: `WHERE deleted_at IS NULL` — partial unique indexes use the same predicate where re-creation after delete is allowed | ADR-007 §E |
| Composite index `(tenant_id, updated_at, id)` (or `(user_id, …)` for user-owned) — serves RLS scope + `updated_since` + cursor pagination in one seek | ADR-007 §E |
| Composite index `(tenant_id, <business_time>, id)` per business-time filter | ADR-007 §E |
| Partial indexes for low-cardinality status filters | ADR-007 §E |

### 7.2 Sensitivity-typed columns (per [ADR-008 §B](../architecture/ADR-008-data-sharing-posture.md))

| Invariant | Source |
| --- | --- |
| Every encrypted column declared at one of: operational / personal (tenant-context) / user-owned / special-category | ADR-008 §B |
| Special-category fields (medical detail, safety-reporter identity) have **no API representation** at any scope | ADR-008 §B |
| Serialisers enforce field-level class against the caller's scope; CI lint checks the typing | ADR-008 §B |

### 7.3 DEK assignment by controller (per [ADR-012 §A](../architecture/ADR-012-cross-tenant-dek-erasure.md))

| Invariant | Source |
| --- | --- |
| Every encrypted row is encrypted under the **owning controller's** DEK — never another controller's | ADR-012 §A |
| Cross-tenant/cross-controller references are stored as the opaque external ID; no DEK crosses the boundary | ADR-012 §B |
| `KeyProvider::for_record(record_kind, controller)` is the only API code that selects a DEK | ADR-012 §A |
| Maintenance records authored by tenant B against A use **B's** DEK; competency evidence logged at tenant A about a pilot uses the **pilot's** DEK | ADR-012 §A |
| Per-controller **artefact signing keys** (Ed25519; webhook signing, consent-grant assertions, transparency-report signatures) are wrapped under the controller's DEK and stored alongside controller metadata — destroying the DEK destroys the artefact key, making historical signatures permanently unverifiable (crypto-shred via DEK lifecycle) | ADR-013 §C |
| Artefact keys are generated **eagerly at controller creation**, not lazily on first signing | ADR-013 §C |

### 7.4 Audit & event stores (per [ADR-009](../architecture/ADR-009-event-streams-and-retention.md))

| Invariant | Source |
| --- | --- |
| `audit_events`: INSERT-only grant for `app_api`; hash-chained with chains partitioned **per-tenant + per-user + platform** | ADR-004 §D + ADR-009 §C |
| `audit_events` rows reference actors/resources by opaque UUID — **never PII** in metadata | ADR-004 §D |
| `audit_events` declarative range-partitioned monthly on `occurred_at` | ADR-009 §E |
| `events_outbox` declarative range-partitioned monthly on `created_at` | ADR-009 §E |
| `auth_events` and `webhook_deliveries` are separate tables with their own retention (§7.6) | ADR-009 §A |
| Audit-write scope: sensitive Permits + every Deny + every elevated action (bulk-sync, key unwraps, safety de-anon, staff cross-tenant, key rotation, config changes) | ADR-009 §B |
| Platform-chain rows carry `human_reason TEXT` (NOT NULL where `tenant_visible = true`) — the operator-supplied justification surfaced in tenant transparency reports | ADR-010 §J |
| Platform-chain rows carry `tenant_visible BOOLEAN NOT NULL DEFAULT true` — `false` for staff-internal operations that do not affect a specific tenant | ADR-010 §J |
| Platform-chain rows carry `release_at TIMESTAMPTZ NOT NULL DEFAULT now()` — defers the tenant-facing notification under documented legal hold; the audit row itself is created regardless | ADR-010 §J |

### 7.5 ABAC + staff plane (per [ADR-001 §C](../architecture/ADR-001-platform.md) + [ADR-010 §B](../architecture/ADR-010-platform-operator-access.md))

| Invariant | Source |
| --- | --- |
| `Subject.actor_class` enum: `Member` / `Staff` / `System` | ADR-010 §B |
| `Subject.tenant_id` is `Option<Uuid>` — `None` for Staff until elevated | ADR-010 §B |
| `elevations` table (hosted-only): `elevation_id`, `staff_user_id`, `tenant_id`, `purpose`, `justification`, `ticket_ref`, `granted_at`, `expires_at`, `granted_by`, `revoked_at` | ADR-010 §C |
| `mode=self-host` build excludes staff handlers, the admin binary, the elevations table, and the platform chain | ADR-010 §H |

### 7.6 User-consent grants (per [ADR-011](../architecture/ADR-011-user-consent-grant.md))

| Invariant | Source |
| --- | --- |
| `oauth_clients` (registration): `client_id`, `client_secret` (confidential only), `client_type`, `redirect_uris[]`, `requested_scopes[]`, `application_metadata` | ADR-011 §D |
| `oauth_clients.lifecycle_stage` enum: `development` (default at registration) / `submitted` / `verified` — gates authorisation set, consent-screen treatment, and scope availability | ADR-011 §D |
| `oauth_clients.test_users[]`: allow-list of user ids permitted to authorise the app in `development` / `submitted` stages (cap ~25); ignored for `verified` | ADR-011 §D |
| `oauth_clients` `application_metadata` references **first-party-hosted assets** (logo, name, description) stored at registration; homepage URL stored as link only, never fetched at consent time | ADR-011 §D |
| `grants`: `grant_id`, `user_id`, `client_id`, `scopes[]`, `granted_at`, `revoked_at` | ADR-011 §B |
| Refresh tokens are opaque 32-byte values with **rotation** — every use returns a new refresh and invalidates the old; reuse triggers grant revocation | ADR-011 §E |
| Access tokens are 10-minute JWTs carrying `grant_id`, `client_id`, `scope`, `aud` | ADR-011 §E |

### 7.7 Per-stream retention (per [ADR-009 §D](../architecture/ADR-009-event-streams-and-retention.md))

| Stream | Hot (live PG) | Warm (PG, compressed) | Cold (object store, Parquet) | Archive (Object Lock) |
| --- | --- | --- | --- | --- |
| `audit_events` | 90 days | 1 year | 5 years | until 7y, then permanent deletion |
| `events_outbox` | dispatched + 90 days | — | — | (no cold tier — not a fact source) |
| `auth_events` | 30 days | 2 years | — | — |
| `webhook_deliveries` | 30 days | — | — | — |

### 7.8 DB roles (per [ADR-002 §F](../architecture/ADR-002-release-deployment.md) + [ADR-003](../architecture/ADR-003-db-migrations.md) + [ADR-010 §I](../architecture/ADR-010-platform-operator-access.md))

| Role | Privileges | Used by |
| --- | --- | --- |
| `app_migrator` | DDL + DML on application schema | Migration Job only |
| `app_api` | DML; **INSERT, SELECT** on `audit_events` only — never UPDATE/DELETE; no DDL | Running API pods (`apps/api`) |
| `app_platform` | DML scoped to platform-plane operations; INSERT, SELECT on `audit_events` (writes to platform chain only); sets `app.actor_class = 'platform'` session GUC; **hosted-only**, not granted on self-host | Staff binary (`apps/admin`, ADR-010 §I) |
| `app_read_only` | SELECT on application schema | Read replicas, analytics jobs |
| `app_backup` | SELECT for backup workflows | Backup Job |

---

## Sources

- UI/UX source of truth: a pre-production design prototype — ~42
  self-contained React/JSX views + a `system.css` design system, produced
  on a design canvas before implementation. Internal artefact kept outside
  this repository; mechanically ported to Maud templates per [ADR-020](../architecture/ADR-020-mash-frontend-architecture.md) (supersedes the original SvelteKit destination in [ADR-001 §B](../architecture/ADR-001-platform.md)).
- [ADR-001 — Platform architecture](../architecture/ADR-001-platform.md)
- [ADR-002 — Release and deployment](../architecture/ADR-002-release-deployment.md)
- [ADR-003 — Database migration discipline](../architecture/ADR-003-db-migrations.md)
- [ADR-004 — Defence in depth](../architecture/ADR-004-defence-in-depth.md)
