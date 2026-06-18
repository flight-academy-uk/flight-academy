# ADR-001 — Platform architecture

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-28 |
| **Deciders** | @ICreateThunder |
| **Tags** | platform, foundation, api, auth, encryption, safety, telemetry |
| **Supersedes** | (none) |

## Context

Flight Academy is a multi-tenant aviation platform serving three operationally distinct tenant types — flight schools (UK CAA / EASA Approved Training Organisations, "ATOs"), maintenance organisations (UK CAA / EASA Part 145), and airfield operators — plus individual pilots who may belong to multiple organisations or none. The platform handles regulated data including medical certificates, training progress, professional credentials, regulatory submissions, and safety occurrence reports.

### Tenant and user model

The terms used throughout this document:

- **User** — an individual person. Always has authentication identity (email, passkeys, paired devices). May be a member of zero, one, or many tenants. Personal data — own logbook, own medical certificates, own pilot ratings — is owned by the user, not by any tenant.
- **Tenant** — an organisation (flight school, maintenance organisation, airfield operator). Has many users (members). Operational data — fleet, bookings, maintenance jobs, safety occurrences — is owned by the tenant, not by the users.
- **Membership** — the relationship between a user and a tenant, carrying role and attribute data (e.g., a user may be a student at one tenant and an instructor at another).

Concretely: a CPL holder might be a student pilot at one flight school, an instructor at a second, hold maintenance approvals via a Part 145 organisation, and keep a personal logbook independent of all three. Each of those associations is a separate `membership`; the personal logbook is at the user level. The platform's data model and encryption scheme honour both isolation levels — see decision D for how per-tenant and per-user encryption keys differ.

The project is published under AGPL-3.0-only. It is built and run by a single maintainer at the time of writing, with the explicit intent to scale to a multi-maintainer governance model once sustained external contribution materialises (see [GOVERNANCE.md](../../GOVERNANCE.md)). The same source produces two consumption modes: a hosted SaaS run by the project maintainer, and a self-hosted deployment that any operator may run under the AGPL.

The constraints that shaped these decisions:

- **Regulatory** — UK CAA CAP 382 (occurrence reporting), EU 376/2014 (mandatory occurrence reporting), GDPR (data subject rights, lawful basis, breach notification), EASA Part-FCL (licensing data), EASA Part-145 (maintenance documentation).
- **Operational** — solo maintainer; small ARM EC2 nodes; tight per-pod memory budget; no full-time SRE.
- **Ethical** — privacy-by-default; no surveillance; transparency in design; long-term thinking; restraint in scope. See [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md), particularly instrument 28 (truth in what we say and ship) and instruments 35–36 (restraint, read across to scope and dependencies).
- **Economic** — fixed-cost compute preferred over per-request scaling to bound billing-attack exposure; AWS the chosen primary cloud, with portability to other clouds preserved where reasonable.
- **Community** — AGPL-licensed, public from commit one, contributions accepted via DCO sign-off + signed commits + squash-only PR merge.

This ADR records the foundational architecture decisions that bind everything else. Detailed release and deployment topology lives in [ADR-002](ADR-002-release-deployment.md); database migration discipline in [ADR-003](ADR-003-db-migrations.md) (forthcoming); defence-in-depth in [ADR-004](ADR-004-defence-in-depth.md) (forthcoming).

## Deployment context

The architecture documented here is designed for the following deployment environment, though the AGPL self-host story is a first-class concern:

- AGPL-3.0 source, public on GitHub at `flight-academy-uk/flight-academy` from commit one.
- Hosted SaaS on K3s running on AWS EC2 (ARM, eu-west-2). Cloudflare DNS + Email Routing in front; AWS S3 + CloudFront + WAF + NLB for the data path; CloudNativePG operator (CNPG) for Postgres; MinIO for object storage.
- Self-host supported via the same release artefacts — single Rust binary with optional embedded static frontend, plus an official Helm chart, plus a `docker-compose.yaml` for single-VPS deployment.

Deployment specifics (image registry, GitOps engine, canary criteria, install script pattern) live in ADR-002.

## Decision

The platform comprises eight load-bearing decisions, labelled A–H. They are interdependent — for instance, the no-telemetry principle (H) shapes how observability is exposed; the encryption-at-rest model (D) interacts with the safety-reporting design (G). Each subsection states the choice in one sentence and then expands.

### A. API style — REST + OpenAPI

**Decision: HTTP/JSON REST with an OpenAPI 3.1 specification derived from Axum handlers via the `utoipa` crate.**

The API is structured as resource-oriented HTTP endpoints returning JSON. The OpenAPI spec is generated from handler annotations at compile time and committed alongside the code so contributors and consumers can diff it per pull request. Client SDKs (web and Flutter mobile) consume the same spec; we do not hand-write SDK code.

Resource boundaries follow the operational mental model of tenants — `/api/v1/tenants/{tenant}/aircraft/{tail}`, `/api/v1/tenants/{tenant}/flight-logs`, `/api/v1/tenants/{tenant}/safety/occurrences` — with the tenant context resolved at the edge from subdomain and validated against the JWT claim (see decision F). Cross-tenant queries are not part of the public API; tenants may belong to multiple organisations but always operate within one tenant context per request.

Versioning is path-based (`/api/v1/`, `/api/v2/`) rather than header-based, because path versioning is easier for self-hosters to reason about and easier to cache. Backward-compatible changes (new fields, new endpoints) do not require a version bump; breaking changes do.

Internal service-to-service communication is not yet present (the project is a monolith). When it is, gRPC may be reconsidered for internal traffic; external API stays REST.

### B. Frontend components — bits-ui headless + Tailwind + design tokens

**Decision: `bits-ui` for accessible headless primitives, Tailwind CSS for utility-first styling, white-label tenant customisation via CSS custom properties bound to per-tenant settings.**

`bits-ui` provides accessible (keyboard, focus, ARIA) headless primitives without imposing visual decisions; Tailwind builds on top with utility classes. The Claude-extracted design components in the archive directory are JSX; they convert to Svelte structurally with minimal rework.

White-label customisation works via CSS custom properties: `--color-brand`, `--color-surface`, `--color-text`, etc., are declared at the document root and overridden per tenant from a settings JSONB column. Tenant-specific stylesheets are not generated server-side; the customisation is purely a token override at render time.

The frontend is a SvelteKit application using `adapter-static` to produce a fully static build. Marketing pages, authentication flows, and authenticated app shells all live in one codebase using route groups (`(marketing)`, `(auth)`, `(app)`). Routes that need authenticated data fetch from the API; the HTML/JS/CSS shell is static and cacheable.

For self-hosted deployment, the same SvelteKit build is embedded in the Rust binary via the `rust-embed` crate, gated by a Cargo feature flag. See ADR-002 §H.

### C. Authorisation — Hand-rolled ABAC in `fa-auth`

**Decision: Attribute-Based Access Control implemented as a Rust trait + decision enum in the `fa-auth` crate, with policy evaluation invoked explicitly from handlers via a tower middleware that injects the authenticated subject.**

The shape:

```rust
// Illustrative; final API may differ
pub trait Policy {
    fn permit(
        subject: &Subject,
        action: Action,
        resource: &Resource,
    ) -> Decision;
}

pub enum Decision {
    Permit,
    Deny { reason: String },
    NotApplicable,
}

pub struct Subject {
    pub user_id: Uuid,
    pub tenant_id: Uuid,
    pub roles: BTreeSet<Role>,
    pub attributes: SubjectAttributes,   // medical_class, ratings, instructor_seniority, etc.
}

pub struct Resource {
    pub tenant_id: Uuid,
    pub kind: ResourceKind,
    pub owner: Option<Uuid>,
    pub attributes: ResourceAttributes,
}
```

ABAC is preferred over plain RBAC because aviation roles have attributes that materially affect what a user can do — an instructor with a CFI rating can sign off solo dispatch; one without cannot. Encoding that purely as roles forces an explosion of fine-grained role names; ABAC keeps the role set small and uses attributes to decide.

Policy decisions are logged to the audit table (decision G in ADR-004, forthcoming). Every `Permit` is recorded with subject, action, resource, and decision rationale; every `Deny` with the reason.

We do not adopt Cedar, Open Policy Agent, or Casbin at this stage. Those are appropriate when tenant administrators need to author policies at runtime through a UI; we have no such requirement and the operational cost (running OPA as a sidecar, maintaining policy bundles, version-managing them) is not justified. If that requirement emerges, Cedar would be the migration target — it has a clear semantics, an embeddable evaluator, and is designed for SaaS multi-tenant scenarios.

### D. Encryption at rest — Envelope encryption (DEK + KEK), per-tenant

> **Refined by [ADR-022](ADR-022-pluggable-aead.md)** — AEAD algorithm choice is now pluggable; AES-256-GCM-SIV is the default for new writes; ChaCha20-Poly1305 and AES-256-GCM also ship for operator selection and forward migration. AES-256-GCM remains in force for any ciphertext written under §D as originally specified. The envelope-encryption posture (DEK + KEK, per-tenant, crypto-shred) is unchanged.

**Decision: Three-layer encryption — CNPG disk-level + per-tenant column-level AEAD via envelope encryption (DEK wrapped by KEK in KMS) + pgcrypto where searchable-blind columns are needed. GDPR right-to-erasure backed by crypto-shredding of the per-tenant DEK.**

Sensitive columns — medical certificate details, address, passport number, safety occurrence reporter identity — are encrypted with AES-256-GCM using a Data Encryption Key (DEK). For tenant-owned data (a school's fleet records, an airfield's PPR submissions, etc.) the DEK is per-tenant. For user-owned data (an individual pilot's personal logbook, medicals, ratings — irrespective of any tenant membership) the DEK is per-user. The DEK itself is encrypted by a Key Encryption Key (KEK) held in AWS KMS, age (for self-hosters who prefer file-based key management), or SOPS (for the development environment).

The wrapped DEKs are stored alongside the owning entity: `tenants.dek_wrapped` for tenant-scoped data, `users.dek_wrapped` for user-scoped personal data. On request, the API unwraps the appropriate DEK via the KMS provider, caches the plaintext DEK in memory for the request lifetime, performs the AEAD encryption or decryption, and drops the plaintext when the request completes.

Why envelope encryption:

| Capability | Single-key encryption | Envelope encryption |
| --- | --- | --- |
| Rotate master key | Re-encrypt every row | Rewrap N DEKs, ciphertext untouched |
| Crypto-shred one tenant | Impossible | Delete that tenant's wrapped_dek |
| KMS rate limits | One call per byte | One call per request |
| Per-tenant access audit in KMS | Useless (one key) | Per-tenant unwrap log |
| Blast radius of memory disclosure | Everything | One request |

The crypto-shred property is GDPR Article 17 (Right to Erasure) gold. Deleting a tenant's or user's `dek_wrapped` row renders every encrypted column owned by that tenant or user mathematically unrecoverable — even from backups that have already replicated. This is the durable solution to "we need to demonstrate the data is gone" when row-level deletes alone cannot reach all replicated state. An individual pilot exercising their right to erasure has their user-level DEK shredded; an organisation leaving the platform has its tenant DEK shredded; both operations are atomic and effective against backup state.

The `fa-store` crate provides the `KeyProvider` trait with implementations for each backend. The same trait powers `EncryptedString`, `EncryptedJson`, and similar wrapper types that integrate with the ORM. Application code reads and writes plaintext; the wrapper handles wrap/unwrap transparently via the tenant context.

Safety occurrence reporter identity uses a **separate per-tenant DEK** ("safety key") distinct from the general tenant DEK; only the `safety_officer` role can request unwrap of that key. See decision G.

### E. Integrations — Single `fa-integrations` crate with adapter trait per category

**Decision: One Rust crate (`fa-integrations`) holds all external service integrations. Adapter traits per category — `AccountingProvider`, `PaymentProvider`, `BankingProvider`, `AviationDataProvider` — let multiple providers coexist behind a uniform interface.**

Providers per category (initial scope):

| Category | Trait | Providers |
| --- | --- | --- |
| Accounting | `AccountingProvider` | Xero, QuickBooks |
| Payments | `PaymentProvider` | Stripe, GoCardless |
| Banking (UK Open Banking) | `BankingProvider` | TrueLayer, GoCardless Bank Account Data |
| Aviation data | `AviationDataProvider` | UK CAA NOTAM, EUROCONTROL, Met Office DataPoint |

The trait shape captures the shared concepts — for accounting: charts of accounts, invoices, payments, sync state; for payments: charge, refund, customer, mandate; for banking: account linking via OAuth, transaction fetch (AISP), payment initiation (PISP), payee verification.

Why a single crate rather than one per provider:

- Shared error types (`IntegrationError`, retry policy, idempotency), shared signature verification primitives for webhooks
- Easier to keep adapter implementations consistent
- Cargo features (`xero`, `stripe`, `truelayer`) toggle which adapters compile in — self-hosters can omit those they don't use

UK Open Banking (PSD2) requires either FCA authorisation as an AISP/PISP, or use of an authorised aggregator who carries that authorisation. We integrate via aggregators — TrueLayer and GoCardless Bank Account Data are both FCA-authorised — never via direct bank API. Self-hosters who want their own banking integrations may add provider implementations downstream; we will not accept upstream PRs for direct bank integrations because of the regulatory burden.

Webhook receivers are sub-modules per provider, each with HMAC signature verification before payload dispatch. Idempotency keys (Stripe-style) are persisted in `webhook_events` so replays are safe.

SCA (Strong Customer Authentication) consent for Open Banking must be renewed every 90 days; the integrations crate exposes a background job that warns operators 7 days before expiry.

### F. Auth session — HttpOnly cookie + opaque refresh token in DB, passwordless

**Decision: Short-lived JWT access token in an HttpOnly + SameSite=Lax cookie. Opaque refresh token persisted server-side with explicit revocation. Mobile clients use Bearer tokens with the same refresh mechanism. No passwords are ever stored.**

The authentication flows:

- **Magic link** — user enters email; server emails a single-use link with 10-minute expiry; clicking establishes a session.
- **Passkey / WebAuthn** — registered devices authenticate via FIDO2; supported on all modern browsers and OS keychains.
- **Push notification to a paired device** — for users who have at least one paired device with the Flutter app, an "approve sign-in" push arrives during web login.

Session tokens:

- **Access JWT**: 10-minute expiry, signed with a tenant-aware signing key (Ed25519). Stored in HttpOnly + SameSite=Lax cookie for web; held in secure storage on mobile. Contains `sub` (user ID), `tenant` (tenant context), short list of roles, no PII.
- **Refresh token**: 30-day expiry, opaque (random 32 bytes), persisted in `refresh_tokens` table with revocation list. Web stores it in a separate HttpOnly cookie; mobile in platform secure storage.

CSRF protection on the web side uses a double-submit cookie pattern combined with Origin header validation. Tenant context resolves from the subdomain at the edge and must match the JWT's `tenant` claim — mismatch returns 403.

Why no passwords:

- The single largest class of credential breach is reuse of compromised passwords
- Aviation operators expect modern auth; instructors and engineers find password rotation hostile
- Passwordless flows are well-supported across the platforms we target (web, iOS, Android)
- Eliminates password reset, password complexity rules, password leak monitoring as ongoing operational concerns

### G. Safety reporting / MOR — Mandatory in v1, anonymisable, ECCAIRS2-compatible

**Decision: Safety occurrence reporting (MOR per EU 376/2014 and CAP 382) ships in v1. Reporter identity is encrypted with a per-tenant safety key separate from the general tenant DEK; only the `safety_officer` role can decrypt. Export format matches ECCAIRS2 (the UK CAA standard since 2025).**

The data model:

- `safety_occurrences (id, tenant_id, occurred_at, location, aircraft_id?, category, severity, narrative, status, reporter_id_encrypted, created_at, updated_at)`
- `safety_attachments` linked to occurrences via `fa-store` (photos, EFB screenshots, supporting documents)
- State machine: `draft → submitted → under_review → reported_to_caa → closed`

The `reporter_id_encrypted` column holds an AEAD-encrypted user ID. Decryption requires both the safety key (separate per-tenant DEK) and an authorisation check that the requester has the `safety_officer` role. Every decryption is logged to the audit trail along with the requester's user ID, timestamp, and the occurrence being de-anonymised.

Reporters may submit anonymously by default; the form allows toggling reveal-on-submit if the reporter prefers attribution. This is the Just Culture principle in practice — reporters who fear retaliation can still report; the safety officer can still investigate; nobody else can identify the reporter.

The mobile app is the primary reporting surface, because the moment of reporting is often near the aircraft, not at a desk. Offline-first capture queues a draft for sync when network returns.

Cross-link to maintenance: when a reporter ticks "equipment defect", the occurrence creates an associated maintenance defect record linked back to the affected aircraft and visible to the maintenance team. The reporter identity remains protected.

Export format: ECCAIRS2-compatible JSON for submission to the UK CAA. CAA switched from ECCAIRS to ECCAIRS2 in 2025; we support the current format.

Retention: 5 years minimum per CAP 382 §6. The `reporter_id_encrypted` column may be additionally pseudonymised at year 5 if the operator wishes to retain the occurrence data beyond reporter identity retention.

### H. No telemetry, no phone-home — non-negotiable

**Decision: Flight Academy code never sends any signal — diagnostic, telemetric, usage, error, or otherwise — to any Flight Academy-controlled endpoint. Self-hosted instances are observable only to their operators. This is a contribution gate; pull requests adding telemetry will not be merged.**

Concrete implications:

- **Metrics** are exposed at `/metrics` in Prometheus exposition format. The operator scrapes this endpoint, or doesn't. There is no hardcoded scrape target.
- **Traces** export via OTLP. The endpoint is configured via standard OpenTelemetry environment variables (`OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_HEADERS`). If unconfigured, traces drop silently.
- **Logs** write to stdout in JSON. The operator collects them via whatever pipeline they choose (Vector, Fluent Bit, journald shipping, etc.), or doesn't.
- **No update-check ping.** Operators learn about new releases by subscribing to GitHub Releases, RSS, or whatever notification mechanism they choose.
- **No error aggregator pre-wired.** Sentry, Bugsnag, Rollbar — none are integrated. Operators may configure their own.
- **No "anonymous usage statistics"** opt-in, opt-out, or otherwise. There is no anonymous data collection because the data being anonymous does not make the collection acceptable.
- **No telemetry in the mobile app either.** Crash reports stay on-device unless the user explicitly chooses to attach them to a bug report.

The principle aligns with the restraint and transparency of [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md), though no-surveillance is a project stance rather than one of its numbered instruments. It is also enforced as a code-review gate: PRs that introduce any phone-home behaviour, however well-intentioned, are rejected.

The trade-off — that we have no visibility into the wild fleet of self-hosted instances — is accepted in exchange for the trust property. Adoption signals come from indirect channels: GitHub stars, ghcr.io image pull counts (which only count operators who pull from our registry rather than mirroring), GitHub Discussions activity, conference talks. These are sufficient for the project's purposes.

Section 13 of AGPL-3.0 (modifications shared when running networked) is the only "social contract" governing modifications; it has no technical enforcement mechanism in our code, and will not gain any.

## Consequences

### Positive

- **Trust by design** — the no-telemetry stance, the encryption-at-rest design with crypto-shredding, the audit log of every authorisation decision, and the separation of safety reporter identity behind a distinct DEK together establish a default posture of "the operator and the data subjects are in control." This is durable and verifiable, not a marketing claim.
- **Single source for clients** — OpenAPI generation from Axum handlers means the spec cannot drift from the implementation; client SDKs derive deterministically.
- **GDPR Article 17 has a real answer** — crypto-shredding the tenant DEK renders backup-state encrypted data unrecoverable. We do not have to handwave about "we delete it eventually from backups."
- **Self-hosters get the complete product** — no feature gating in the OSS code, no hidden enterprise edition. The same binary, the same spec, the same Helm chart.
- **Passwordless auth removes a class of incident** — credential-stuffing, password reuse, weak password policy enforcement all become non-applicable.
- **ABAC scales with attributes, not roles** — aviation roles have meaningful sub-states (current vs not current; instructor seniority levels; CFI vs II); ABAC encodes these without exploding role names.
- **Safety reporting from v1 honours Just Culture** — anonymous-by-default reporting with safety-officer-only de-anonymisation prevents the reporting chilling effect that punitive cultures produce.
- **The architecture is portable** — Postgres + S3-compatible objects + standard K8s + OCI containers; the same code runs on AWS, GCP, Azure, on-prem, or a single VPS.

### Negative

- **No adoption analytics** — we genuinely do not know how many self-hosters exist, what versions are deployed, or which features are used. Indirect signals only.
- **Crypto-shredding requires operational discipline** — backups must be encrypted with the same DEKs to make crypto-shredding effective; rotating a tenant out of crypto-shredding into pseudonymisation must be deliberate.
- **ABAC policy authoring is in Rust, not a config language** — adding new policies requires a code change, review, release. Non-developers cannot author policies. Acceptable for the project's scale; would need revisiting if tenant admins need to write policies.
- **Webhook signature verification is per-provider** — every integration adapter must independently implement signature verification correctly. We mitigate via shared helpers in `fa-integrations`, but the surface area is real.
- **Passwordless flows require email reachability** — magic-link auth assumes the user's email is working. A user whose email is broken cannot self-recover; an operator-side recovery flow is required.
- **The Rust + Axum + Postgres + Svelte + Flutter stack is heterogeneous** — contributors need familiarity with multiple ecosystems. Mitigated by clear separation between layers and conventional patterns within each.
- **Mandatory safety reporting adds schema and code in v1** — modest cost; the alternative (deferring to v1.5) would have meant a major migration when added, which is worse.

### Neutral

- **OpenAPI 3.1 over GraphQL** — gives up GraphQL's selective-field flexibility in exchange for simpler caching, simpler rate-limiting, smaller attack surface. Most clients want consistent response shapes anyway.
- **Single integrations crate** — couples integration adapters via shared types; if integrations grow to dozens, we may split. Not a problem at current scale.
- **Magic link expiry of 10 minutes** — tight enough to limit replay; loose enough to survive an email queue delay. Adjustable per deployment via config.
- **Audit log retention** — undecided in this ADR; will be specified in ADR-004 (defence in depth). 7 years is the working assumption based on regulatory retention norms.

## Alternatives considered

### A. GraphQL instead of REST

Rejected because GraphQL widens the attack surface (query cost analysis, depth limiting, introspection lockdown), complicates rate-limiting and caching, and offers little benefit for an API whose clients are predominantly our own SvelteKit and Flutter apps. Would reconsider if we ever exposed a public developer API for third-party integrations where clients need flexible field selection.

### B. shadcn-svelte full component library

Rejected for bundle size (~150 KB of imposed conventions) and the friction of overriding its visual decisions for white-label customisation. `bits-ui` headless gives the same a11y story at ~30 KB and full control of presentation. Would reconsider if a much larger frontend team made consistency more valuable than control.

### C. Cedar, OPA, or Casbin for authorisation

Rejected for current scale. Cedar is the migration target if tenant admins ever need to author policies through a UI; OPA adds an out-of-process dependency for evaluation that is not justified for an in-process Rust application; Casbin is workable but the Rust binding is less mature than the Go original. Hand-rolled trait + enum is small, fast, and inspectable.

### D. Single master key for column encryption

Rejected because it makes per-tenant crypto-shredding impossible, makes per-tenant access auditing useless, and makes master key rotation require re-encrypting every row. Envelope encryption costs one KMS unwrap per request — comparable performance, dramatically better properties.

### E. Direct bank API integration instead of aggregators

Rejected because the FCA AISP / PISP regulatory burden makes direct integration impractical for an OSS project. Aggregators (TrueLayer, GoCardless Bank Account Data) carry the FCA authorisation; we integrate with them.

### F. Password-based authentication with TOTP fallback

Rejected because passwords introduce a class of operational problems (rotation, complexity, reuse, breach monitoring) that passwordless flows eliminate. The supporting technology (passkeys, magic links, push) is mature in 2026; users find it easier, not harder.

### G. Deferring safety reporting to v1.5

Rejected for two reasons. First, EASA-regulated tenants legally require occurrence reporting; building it later means tenants who need it cannot use the product until then. Second, the data model for occurrences is structurally distinct from operational data — building it later means a significant migration. The cost of inclusion is modest; the cost of omission is meaningful.

### H. Anonymous usage telemetry with opt-out

Rejected because we cannot make telemetry trustworthy by adding consent flags. The minute we ship telemetry, even opt-out, we have to maintain it, secure it, store it, defend it against subpoena, and explain its absence after an incident. The simpler, more defensible posture is to ship no telemetry at all. The cost — no adoption analytics — is borne by us, not by users. That is the correct distribution of cost.

## References

### Related ADRs

- [ADR-002 — Release and deployment](ADR-002-release-deployment.md) — registry, GitOps, canary, install pattern
- [ADR-003 — Database migration discipline](ADR-003-db-migrations.md) — forward-only, expand-contract, Migration Job (forthcoming)
- [ADR-004 — Defence in depth](ADR-004-defence-in-depth.md) — billing-attack circuit breaker, rate limiting, audit log, honeypots (forthcoming)

### Project documents

- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — its restraint instruments (35–36) and the project's no-surveillance stance inform decisions H and E
- [SECURITY.md](../../SECURITY.md) — threat model, PGP key, supply-chain posture
- [GOVERNANCE.md](../../GOVERNANCE.md) — BDFL transition, ADR process, commercial/OSS separation
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — DCO, signed commits, conventional commit titles

### External standards and regulations

- AGPL-3.0-only — [gnu.org/licenses/agpl-3.0.html](https://www.gnu.org/licenses/agpl-3.0.html)
- GDPR — Regulation (EU) 2016/679, particularly Articles 15 (access), 17 (erasure), 25 (data protection by design)
- UK CAA CAP 382 — Mandatory Occurrence Reporting Scheme
- EU 376/2014 — Mandatory Occurrence Reporting in civil aviation
- EASA Part-FCL — Pilot licensing
- EASA Part-145 — Maintenance organisation approvals
- OpenAPI Specification 3.1 — [spec.openapis.org/oas/v3.1.0](https://spec.openapis.org/oas/v3.1.0)
- WebAuthn Level 3 — W3C recommendation
- ECCAIRS2 — European Co-ordination Centre for Accident and Incident Reporting Systems, version 2

### Tooling and libraries

- `axum` — HTTP framework
- `utoipa` — OpenAPI derivation
- `sqlx` — Postgres async driver with compile-time query checking
- `webauthn-rs` — WebAuthn/passkey implementation
- `rust-embed` — compile-time static asset embedding
- `bits-ui` — accessible Svelte primitives
- `tower-http` — HTTP middleware
- `tower_governor` — rate limiting middleware
- CloudNativePG (CNPG) — Postgres operator for Kubernetes

## Notes

This ADR is the architectural keel. Subsequent ADRs refine specific areas — release/deployment (002), database migrations (003), defence-in-depth (004), and any future ADRs as the project grows — but they all rest on the decisions documented here. If a future ADR materially changes any of A–H, that ADR must explicitly note which decision it supersedes and update this document's status to "Superseded by ADR-XXX."

Some terms used here without further definition:

- **AEAD** — Authenticated Encryption with Associated Data; AES-256-GCM and ChaCha20-Poly1305 are the canonical choices.
- **DEK / KEK** — Data Encryption Key / Key Encryption Key (envelope encryption terms).
- **CNPG** — CloudNativePG, the Postgres operator.
- **MOR** — Mandatory Occurrence Report (the safety reporting scheme).
- **PSD2 / AISP / PISP** — Payment Services Directive 2; Account Information Service Provider; Payment Initiation Service Provider (UK Open Banking).
- **Just Culture** — the aviation safety principle that error reporting must not be punitive if errors are to surface.
