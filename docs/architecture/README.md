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
