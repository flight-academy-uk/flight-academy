# Architecture Decision Records

This directory contains the Architecture Decision Records (ADRs) for Flight Academy. Each ADR captures a single significant decision, the context that led to it, and the consequences (positive, negative, neutral) that follow.

ADRs are reviewed via the [governance process](../../GOVERNANCE.md#architectural) and merged with status `Accepted`. They are not rewritten — superseded ADRs are linked to the new ADR that replaces them and remain as historical record.

## Index

| # | Title | Status | Date |
| --- | --- | --- | --- |
| [ADR-001](ADR-001-platform.md) | Platform architecture — API style, components, ABAC, envelope encryption, no telemetry, safety/MOR | Accepted | 2026-05-28 |
| [ADR-002](ADR-002-release-deployment.md) | Release and deployment — two repos, ghcr+ECR, Flux + Flagger, canary, install pattern, embedded-static | Accepted | 2026-05-28 |
| [ADR-003](ADR-003-db-migrations.md) | Database migration discipline — forward-only, expand-contract, Migration Job, schema validation in CI | Accepted | 2026-05-28 |
| [ADR-004](ADR-004-defence-in-depth.md) | Defence in depth — billing-attack circuit breaker, rate limiting, audit log, honeypots | Accepted | 2026-05-28 |
| [ADR-005](ADR-005-workspace-layout.md) | Workspace and crate layout — `flight-academy-*` crates, `apps/` binaries, `fa-*` reconciliation | Accepted | 2026-05-29 |
| [ADR-006](ADR-006-api-contract.md) | API contract conventions — code-first OpenAPI pipeline, versioning, addressing, idempotency, errors, webhooks | Accepted | 2026-05-29 |
| [ADR-007](ADR-007-sync-filtering-deletion.md) | Incremental sync, list filtering, and deletion semantics — `updated_since` feed, curated filters, soft-delete tombstones, delete-vs-erase | Accepted | 2026-05-29 |
| [ADR-008](ADR-008-data-sharing-posture.md) | API data-sharing posture and scope model — integration-first, minimisation-first; gated bulk sync; special-category never leaves | Accepted | 2026-05-29 |
| [ADR-009](ADR-009-event-streams-and-retention.md) | Domain-event streams, audit scope, and retention — three stores + webhook dispatcher; sensitive Permits + every Deny; per-tenant chains; monthly partitioning; tiered Parquet cold storage | Accepted | 2026-05-29 |
| [ADR-010](ADR-010-platform-operator-access.md) | Platform-operator / staff cross-tenant access — separate internal surface; ABAC `Subject.actor_class=Staff`; just-in-time time-boxed justified break-glass; hardware passkey + corporate IdP; platform-chain audit; disabled on self-host | Accepted | 2026-05-29 |
| [ADR-011](ADR-011-user-consent-grant.md) | User-consent grant flow — OAuth 2.1 + PKCE; user-owned data scopes; short JWT + opaque rotating refresh; consent UX; revocation; app is GDPR Art. 28 processor | Accepted | 2026-05-29 |
| [ADR-012](ADR-012-cross-tenant-dek-erasure.md) | Cross-tenant DEK assignment & erasure semantics — controller-owner rule; opaque cross-controller references; dangling-pseudonym on erasure; new `resource.reference-erased` event | Accepted | 2026-05-29 |
| [ADR-013](ADR-013-auth-keys.md) | Auth keys and signing infrastructure — two key universes (session per plane, artefact per controller); Ed25519; `kid` rotation; KMS in hosted, `age`/HKDF on self-host; JWKS publication | Accepted | 2026-06-02 |
| [ADR-014](ADR-014-frontend-architecture.md) | Frontend architecture — two SvelteKit codebases (tenant + staff) + one Flutter; shared `apps/web-ui` primitives; design tokens as web↔mobile bridge; embedded-static handshake; wrapped generated clients | Accepted | 2026-06-02 |
| [ADR-015](ADR-015-csp-static-build.md) | CSP and static-build reconciliation — hash-based CSP for static surface; per-request nonce for sensitive routes; per-request nonce + `'strict-dynamic'` for staff plane; inline-style attributes denied | Accepted | 2026-06-02 |
| [ADR-016](ADR-016-compliance-baseline.md) | Compliance baseline and certification commitments — applicable-by-law (UK CAA, EASA, ICAO, UK/EU GDPR); design-aligned (Cyber Essentials Plus, ISO 27001 + 27018, SOC 2, WCAG 2.2 AA); operating standards; explicit out-of-scope (DO-178C, FedRAMP, HIPAA, NIS2); self-host accountability split | Accepted | 2026-06-02 |
| [ADR-017](ADR-017-outbound-http-ssrf.md) | Outbound HTTP and SSRF posture — single `OutboundHttpClient` chokepoint with scheme/IP/redirect/timeout policy; DNS-rebinding closed by connect-time re-resolution; NetworkPolicy denies pod egress to private + metadata ranges; AWS IMDSv2 enforced; refines [ADR-004](ADR-004-defence-in-depth.md), extends [ADR-001 §E](ADR-001-platform.md) | Accepted | 2026-06-03 |
| [ADR-018](ADR-018-openapi-emission-format.md) | OpenAPI emission format — JSON at `docs/api/openapi.json` via `to_pretty_json()`; drops `unsafe-libyaml-norway` transitive; refines [ADR-005 §E](ADR-005-workspace-layout.md) and [ADR-006 §A](ADR-006-api-contract.md) (path-only); preserves swap-back path if a safe YAML emitter materialises | Accepted | 2026-06-04 |

## Template

New ADRs start from [ADR-template.md](ADR-template.md). Number sequentially; once a number is assigned to a draft, it does not get reused even if the draft is abandoned (use `Status: Withdrawn` instead).

## Why ADRs

Architectural decisions accumulate context that is hard to recover later: what alternatives were considered, what constraints applied at the time, what trade-offs were accepted. Reading the current code shows *what* the project is; reading the ADRs shows *why*. We want contributors who arrive in 2027 or 2031 to understand the project from its ADRs, not have to reverse-engineer intent from commit history.

## Adding an ADR

1. Copy [ADR-template.md](ADR-template.md) → `ADR-NNN-short-title.md` (next sequential number)
2. Fill in Context, Decision, Consequences, Alternatives
3. Open a draft PR; mark "Ready for review" once complete
4. Discussion happens on the PR
5. Approval + 7-day cooling period during which any maintainer may object (per [GOVERNANCE.md](../../GOVERNANCE.md))
6. Merge with `Status: Accepted`
7. Update this index

## Status values

| Status | Meaning |
| --- | --- |
| Draft | In development; details may change |
| Proposed | PR open; under discussion |
| Accepted | Merged and in force |
| Superseded by ADR-XXX | Replaced; see linked ADR for current decision |
| Deprecated | No longer applicable but not formally replaced |
| Withdrawn | Number assigned but the proposal was abandoned |
