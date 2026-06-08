# Changelog

All notable changes to Flight Academy are documented in this file.

The format is based on [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

#### Architecture decision records

- ADR-001 through ADR-020 accepted, covering: platform choices, release & deployment, database & migrations, defence-in-depth, workspace & crate layout, API contract conventions, incremental sync & filtering, API data-sharing posture, event streams & audit scope, platform-operator (staff) cross-tenant access, user-consent grant flow (OAuth 2.1), cross-tenant DEK assignment & erasure-by-reference, auth keys & signing infrastructure, frontend architecture, CSP & static-build reconciliation, compliance baseline, outbound HTTP & SSRF posture, OpenAPI emission format (JSON over YAML), white-label runtime (brand-as-immutable-asset + sensitive-route preview, with aviation safety carve-outs), MASH frontend architecture (Maud + HTMX + Alpine + Tailwind on Axum; CloudFront edge with ACM wildcard cert + WAF; single Rust binary; aviation safety floor no-JS mandatory; supersedes ADR-014 SvelteKit).
- Living domain model reference at `docs/design/domain-model.md`; operations docs queue at `docs/operations/README.md`.

#### Backend — Rust workspace

- Initial Cargo workspace skeleton per ADR-005: `apps/api` binary; `flight-academy-core`, `flight-academy-auth`, `flight-academy-db`, `flight-academy-test-support` crates.
- Axum HTTP foundation: `/healthz` endpoint, code-first OpenAPI emission via `utoipa-axum`, `emit-spec` subcommand, problem+json error envelope (RFC 9457), `x-request-id` middleware (UUID v7).
- DB foundation: `sqlx` + embedded migrations, `Db` handle with `begin_tenant` (`SET LOCAL ROLE app_api` + `app.current_tenant` GUC), `migrate` subcommand.
- `audit_events` table per ADR-009: range-partitioned by month, INSERT-only at the SQL level (statement triggers — PG 17+ compatible), per-tenant + per-user + platform chain kinds, RLS isolating tenant chains from `app_api`.
- ABAC primitives per ADR-001 §C and ADR-010 §B: `Subject { user_id, actor_class, tenant_id, roles, attributes, elevation }`, `Action`, `Resource`, `Policy` trait, `Decision { Permit, Deny, NotApplicable }`. Concrete policies: `TenantOwnership` (baseline tenant-match gate), `TenantAdministration` (composes ownership + `Role::TenantAdmin`).
- Tenants resource: table with slug-addressing (regex-constrained, partial unique index where not deleted), `tenant_type` CHECK over `('ato','part_145','airfield_operator')`, soft-delete from day one (`deleted_at` + `deletion_reason` consistency CHECK), `(updated_at, id)` watermark index. `GET`/`PATCH`/`DELETE /api/v1/tenants/{slug}` endpoints, ABAC-gated, with atomic UPDATE-plus-audit in a single SERIALIZABLE transaction.
- Hash-chained audit writer per ADR-004 §H + ADR-009 §C: SHA-256 over RFC 8785 JCS canonical bytes (`occurred_at`, `actor_class`, `actor_id`, `tenant_id`, `chain_kind`, `chain_id`, `payload`) concatenated with `prev_hash`; SERIALIZABLE isolation + bounded retry on `SQLSTATE 40001`; `audit::payload_hash` re-derivation helper for verifiers; algorithm-agility via persisted constituent fields.
- Startup pool-role pre-flight (`Db::verify_audit_pool_role`): refuses to start `serve` if the pool's session role lacks `INSERT`/`SELECT` grant on `audit_events` or doesn't bypass RLS — closes the silent-chain-fork failure mode (RLS-subjected role would return empty `prev_hash` lookups, every row becoming a new "first" entry without surface).
- Baseline security headers (ADR-004 §F + OWASP additions): Content-Security-Policy (deny-everything for JSON), Strict-Transport-Security preload, X-Frame-Options DENY, X-Content-Type-Options nosniff, Referrer-Policy strict-origin-when-cross-origin, Permissions-Policy (sensors / camera / mic / geolocation / payment / USB denied), Cross-Origin-Resource-Policy same-origin, Cross-Origin-Opener-Policy same-origin, Cache-Control no-store. Emitted outermost via `entry().or_insert()` so future static-route handlers can supply their own per-surface CSP.

#### Web — pending MASH foundations (per ADR-020)

- `apps/web-ui/tokens/` preserved as design source-of-truth (`tokens.json` + JSON Schema + `tokens.css`); will be consumed by Tailwind `@theme` when the MASH foundations PR lands.
- SvelteKit skeleton (`apps/web/`, root Bun workspace, `apps/web-ui` scripts + styles, root `package.json` / `bun.lock` / Prettier config, `.github/workflows/web-ci.yml`) removed per [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) §P. Web surface will be server-rendered by `apps/api` via Maud + HTMX + Alpine + Tailwind v4. Nothing has been released — this prune happens entirely inside `[Unreleased]`.

#### CI / tooling

- `scripts/check-all.sh` orchestrates `cargo audit`, `cargo deny check`, `gitleaks dir`, `typos`, `cargo fmt --check`, `cargo clippy -D warnings`, `shellcheck`, `actionlint` — same set the CI workflows run.
- CI workflows: `CI` (Rust lint + test on PG 18 service + schema-drift check against committed `crates/flight-academy-db/schema.sql` per ADR-003 §E), `DCO` (inlined sign-off check), `OpenSSF Scorecard`. All actions SHA-pinned with version-trailer comments. MASH web tooling workflow (Tailwind compile + CSS bundle budget per ADR-020 §O) lands with the MASH foundations PR.
- Integration test infrastructure: testcontainers-modules + tokio `OnceCell` PG container + `tokio::Mutex` migration lock; per-test fresh database; superuser pool so RLS is bypassed for seeds while tenant-scoped reads exercise the policy.
- `Dependabot` configured for `cargo`, GitHub Actions, and Docker (anticipated `deploy/docker/`). Patches grouped per ecosystem; majors come as separate PRs. Web npm watcher is re-added with the MASH foundations PR (Tailwind dev dep per ADR-020 §O).

#### Repository scaffolding (preserved from the original entry)

- Licence (AGPL-3.0), contribution policy, code of conduct, code of ethics (the seventy-two instruments), security policy, governance, code owners, changelog, editorconfig, gitignore, issue and pull request templates.

### Security

- All commits to `main` are GPG-signed and DCO sign-off'd (`git commit -s`); the DCO check is required on every PR.
- `cargo-deny` policy enforces AGPLv3-compatible licences and bans known-vulnerable advisories; `cargo audit` runs in CI; `gitleaks` scans the repo before every push.
- OpenSSF Scorecard workflow publishes the project's posture; protected branches enforce signed commits, linear history, and pull-request review.

[Unreleased]: https://github.com/flight-academy-uk/flight-academy/commits/main
