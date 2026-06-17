# Flight Academy

Open-source platform for UK aviation — flight schools, maintenance organisations, airfields.

> **Status:** pre-alpha. ADRs accepted; the walking skeleton, first tenant-scoped read/write API, hash-chained audit trail, baseline security headers, and the web design system have shipped to `main`. First end-user-facing feature lands when passwordless auth is wired.

[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)
[![OpenSSF Scorecard](https://api.scorecard.dev/projects/github.com/flight-academy-uk/flight-academy/badge)](https://scorecard.dev/viewer/?uri=github.com/flight-academy-uk/flight-academy)

## What this is

A multi-tenant platform consolidating the operational paperwork of UK general aviation across three tenant types:

- **Flight schools** — bookings, dispatch, student logbooks, currency tracking, instructor scheduling, examiner audit packs
- **Maintenance organisations** (Part 145) — work orders, AD/SB tracking, ARC reminders, CAMO sign-off chains
- **Airfield operators** — PPR & slot management, NOTAM publishing, ramp board, movement billing

Plus a Flutter mobile companion (iOS + Android, offline-first) for pilots in flight.

Individual pilots may hold accounts independent of any organisation, and may belong to multiple organisations across these tenant types.

## Architecture

| Layer | Choice |
| --- | --- |
| Backend | Rust + Axum, distroless ARM containers |
| Web | Maud + HTMX + Alpine + Tailwind (MASH stack, server-rendered by `apps/api`) |
| Mobile | Flutter (offline-first) |
| Database | PostgreSQL via CloudNativePG, row-level security |
| Objects | MinIO (S3-compatible) |
| Auth | Passwordless — magic link, passkeys, push |
| Mesh / IDS | Istio ambient + Cilium CNI + Tetragon |
| Deploy | K3s satisfying the ADR-021 interface contract (Cilium CNI, Istio ambient, CNPG, S3-compatible object store), Cloudflare edge, Flux + Flagger |

The architectural decision records are at [docs/architecture/](docs/architecture/) — [ADR-001](docs/architecture/ADR-001-platform.md) frames the platform; [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) supersedes the original SvelteKit frontend choice with MASH; [ADR-021](docs/architecture/ADR-021-cdn-front-door.md) refines ADR-001 §A's origin substrate as an interface contract independent of cloud vendor.

## Hosted vs self-host

The same code, two ways to consume:

| | Hosted (flight-academy.app) | Self-host |
| --- | --- | --- |
| Price | Per-tenant subscription | Free under AGPLv3 |
| Operations | We run it | You run it |
| Updates | Continuous | When you upgrade |
| Data residency | EU (UK CAA-compatible under UK adequacy decision) | Wherever you deploy |
| Support | Direct | Community |

**No feature gating.** Self-hosters receive the complete product. The difference is operational, not functional.

## Privacy by default

Flight Academy contains no telemetry. No phone-home, no usage analytics, no error aggregation that ships data anywhere the operator did not configure. A self-hosted instance is observable only to its operator. This is a contribution requirement, not a default — see [docs/architecture/ADR-001-platform.md](docs/architecture/ADR-001-platform.md).

## Status

| Component | State |
| --- | --- |
| Architectural decision records | accepted (ADR-001 through ADR-020) |
| Backend API (Rust, Axum) | in active development — HTTP foundation, ABAC primitives, hash-chained audit trail, tenants resource (read + write) |
| Web (MASH) | in active development — Maud `/` landing page; vendored HTMX 2.x + CSP-safe Alpine; Tailwind compiled at build time; content-hashed `/static/*` URLs; `embedded-static` cargo feature for self-host single-binary distribution per [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) §O |
| Mobile (Flutter) | not started |
| Helm chart for K8s self-host | not started |
| `docker-compose.yaml` for single-host self-host | not started |

A public roadmap will be published once we have one worth sharing. Until then, the [`docs/architecture/`](docs/architecture/) Architecture Decision Records are the most reliable indicator of direction, and the [CHANGELOG](CHANGELOG.md) catalogues what has actually landed.

## Quick start (development)

What currently works locally — a unified dev orchestration command will land later:

```bash
git clone https://github.com/flight-academy-uk/flight-academy
cd flight-academy

# Rust API: build, lint, test (Postgres testcontainers run from cargo test)
cargo build --workspace
cargo test --workspace

# Web: re-added when MASH foundations land per ADR-020 §O (Tailwind compile)

# Pre-push hygiene gate (mirrors CI: lint, audit, deny, gitleaks, ...)
scripts/check-all.sh
```

Full setup guide will be at [docs/development/setup.md](docs/development/setup.md).

## Self-hosting

Once v0.1 lands:

```bash
curl -fsSL https://install.flight-academy.app | bash
```

For the reviewable path (recommended for security-conscious operators):

```bash
curl -fsSL https://install.flight-academy.app -o install.sh
gpg --verify install.sh.sig install.sh   # verify against the published fingerprint in SECURITY.md
less install.sh                          # read what it will do
bash install.sh
```

K8s users — Helm chart at `ghcr.io/flight-academy-uk/charts/flight-academy`. Full guide at [docs/self-hosting/](docs/self-hosting/).

Building the self-host single-binary artefact from source:

```bash
cargo build --release -p flight-academy-api --features embedded-static
```

The `embedded-static` cargo feature bakes every served asset (Tailwind-compiled CSS, vendored HTMX + Alpine bundles, fonts) into the binary via `rust-embed` per [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) §O, so the resulting `flight-academy` binary is fully self-contained — no on-disk `static/` directory required at runtime. The default build (no feature flag) serves `/static/*` from disk via `ServeDir` for hosted-production deployments (where CloudFront fronts the asset path).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md). All contributions require a DCO sign-off (`git commit -s`) and a cryptographic signature, under [AGPLv3](LICENSE). Newcomers welcome — look for the `good-first-issue` label once present.

## Security

Report vulnerabilities via [GitHub Private Security Advisories](https://github.com/flight-academy-uk/flight-academy/security/advisories/new). See [SECURITY.md](SECURITY.md) for the full disclosure policy, threat model, and supported versions.

## Governance

See [GOVERNANCE.md](GOVERNANCE.md) for how decisions are made and how to become a maintainer.

## License

[AGPL-3.0-only](LICENSE). If you run a modified version over a network, you must share your modifications with users of that service. Plain redistribution, modification, and self-hosting are all permitted.
