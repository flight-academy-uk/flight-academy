# ADR-005 — Workspace and crate layout

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | workspace, cargo, crates, naming, structure |
| **Supersedes** | (none) |

## Context

[ADR-002 §A](ADR-002-release-deployment.md) settled the *repository* boundary
(two repos: source + infra). The *internal* layout of the source repo — crate
set, naming, where the apps live — is unspecified. ADR-001 to ADR-003 named
crates incidentally with a short `fa-` prefix inherited from the prototype
(`fa-auth`, `fa-store`, `fa-integrations`, `crates/fa-db/…`). Those references
were illustrative of responsibility, not a ratified scheme.

Forces: unambiguous names (the repo is `flight-academy`, `fa-` reads as noise);
compiler-enforced layering on the small ARM build budget; restraint on crate
count ([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instruments 35–36);
[ADR-002 §H](ADR-002-release-deployment.md)'s single binary with optional
`embedded-static`; truth in the tree (instrument 28).

## Decision

**Single Cargo workspace. Library crates live under `crates/` with verbose
`flight-academy-*` names. Applications live under `apps/`. The API at `apps/api`
is a combined lib + bin crate that gains the embedded frontend through a Cargo
feature.**

### A. Top-level layout

```text
flight-academy/
├── Cargo.toml          # workspace: members = crates/*, apps/api
├── crates/             # Rust libraries; no binaries
│   ├── flight-academy-core/
│   ├── flight-academy-db/
│   ├── flight-academy-auth/
│   ├── flight-academy-aviation/
│   ├── flight-academy-store/
│   └── flight-academy-integrations/
├── apps/
│   ├── api/            # Rust: lib + bin (§D)
│   ├── admin/          # Rust: future staff plane binary (§F; ADR-010 §I)
│   ├── web/            # SvelteKit + Tailwind (ADR-014 §A)
│   ├── admin-web/      # SvelteKit + Tailwind, future staff-plane web (ADR-014 §A)
│   ├── web-ui/         # Bun workspace package — shared Svelte primitives + design tokens (ADR-014 §B)
│   └── mobile/         # Flutter (ADR-014 §A)
├── charts/             # Helm chart sources (ADR-002 §A)
├── docs/
└── docker-compose.yaml # self-host bundle source (ADR-002 §C)
```

Workspace members are `crates/*` and `apps/api`. `apps/admin` lands when
triggered (§F). `apps/web`, `apps/admin-web`, `apps/web-ui`, and
`apps/mobile` have their own toolchains (Bun, Flutter) and are not Cargo
members.

### B. Naming — verbose, no prefix, dir = package = lib

Every Rust library is `flight-academy-<role>`. Directory name = Cargo package
name = library name, so imports are mechanical:

```rust
use flight_academy_core::{Error, Result};
use flight_academy_db::tenant::TenantRepo;
use flight_academy_auth::abac::{Policy, Decision};
```

This **refines** the incidental `fa-*` references in earlier Accepted ADRs:

| Earlier reference | Now |
| --- | --- |
| `fa-auth` (ADR-001 §C) | `flight-academy-auth` |
| `fa-store` (ADR-001 §D) | `flight-academy-store` |
| `fa-integrations` (ADR-001 §E) | `flight-academy-integrations` |
| `crates/fa-db/migrations/` (ADR-002 §F, ADR-003) | `crates/flight-academy-db/migrations/` |
| `crates/fa-db/schema.sql` (ADR-003 §E) | `crates/flight-academy-db/schema.sql` |

The earlier ADRs remain Accepted and unedited; their `fa-*` names are read
through this table.

DB roles (`app_migrator`, `app_api`, `app_read_only`, `app_backup`) and the API-
key prefix `fa_sk_{live,test,sandbox}_` ([ADR-006 §G](ADR-006-api-contract.md))
are **not** crate names and are unaffected by the rename.

### C. Crate set

| Crate | Responsibility | May depend on |
| --- | --- | --- |
| `flight-academy-core` | Shared primitives; canonical `Error` enum + `Result<T>` alias following the Jeremy Chone / Rust10x pattern (`derive_more::From`, `?`, `Display` as `Debug`); `IntoResponse` added by HTTP layer. No I/O, no framework. | (leaf) |
| `flight-academy-db` | sqlx access; migrations; `schema.sql`; RLS-aware repositories (ADR-003). | core |
| `flight-academy-store` | Object storage (MinIO/S3) + envelope-encryption `KeyProvider`, `EncryptedString`, `EncryptedJson` (ADR-001 §D). Two concerns bundled by a shared request-lifetime key cache. A future split to `…-storage` + `…-crypto` is non-breaking; trigger is the first non-store consumer of `KeyProvider` (same extraction discipline as §F's `flight-academy-http-core`). | core |
| `flight-academy-auth` | ABAC (ADR-001 §C), passwordless sessions (ADR-001 §F), WebAuthn/magic-link/push. | core, db, store |
| `flight-academy-aviation` | Aviation logic: EASA logbook + currency, W&B, competency, ECCAIRS2. Pure functions over `core` types; no I/O, no DB, no framework. Boundary vs `core`: `core` is domain-agnostic, `aviation` is aviation-specific. | core |
| `flight-academy-integrations` | External adapters behind per-category traits (ADR-001 §E); Cargo features per provider. | core, store |

Rule the compiler enforces: **no library depends on a web framework.** Axum
and utoipa live only in `apps/api`.

### D. API binary — `apps/api` as lib + bin

`apps/api` is a single crate with both targets:

- `src/lib.rs` — the app builder (utoipa-axum `OpenApiRouter` + `routes!`,
  handlers, middleware, the assembled OpenAPI document). Integration tests
  depend on this.
- `src/main.rs` — thin entrypoint: config, build app, serve (and the
  `migrate` subcommand per ADR-003 §C).

The `embedded-static` Cargo feature ([ADR-002 §H](ADR-002-release-deployment.md))
gates `rust-embed` + a static-asset handler; all API code is identical between
variants.

### E. Generated artefacts

- **Migrations & schema**: `crates/flight-academy-db/migrations/` and
  `crates/flight-academy-db/schema.sql`. The `.sqlx/` query cache sits at the
  workspace root, reviewed like source.
- **Emitted OpenAPI spec**: written by `apps/api` at `docs/api/openapi.yaml`
  (committed; diffed per PR per ADR-006 §A).
- **Generated TS client**: `apps/web/src/lib/api/generated/` (gitignored;
  emitted at build by `openapi-typescript`).
- **Generated Dart client**: `apps/mobile/lib/api/generated/` (gitignored;
  emitted at build by `openapi-generator` `dart-dio`).

Generated clients are never hand-edited; if they need tweaks, the spec or
generator config changes.

### F. Second HTTP-speaking binary: `apps/admin`

The workspace anticipates a second binary, **`apps/admin`**, hosting the
platform staff plane ([ADR-010 §I](ADR-010-platform-operator-access.md)).
Not present in v1; lands when the first non-trivial staff endpoint is
needed.

When it lands, HTTP plumbing in `apps/api` is extracted to a new shared
crate **`flight-academy-http-core`**: Axum middleware (tracing,
request-id, problem+json, tenancy guard), the `IntoResponse` impl for
the canonical `Error` enum (the enum itself stays in `flight-academy-core`
per §C — only the HTTP-layer rendering moves), `RequestContext`,
ABAC primitives, utoipa registration helpers. Both binaries depend on
it; neither depends on the other. Each emits its own OpenAPI spec; the
staff spec is internal
([ADR-010 §I](ADR-010-platform-operator-access.md)).

**Extraction trigger.** The first non-trivial endpoint in `apps/admin`.
Earlier is premature — the shared surface's shape is only knowable once
two consumers exist. Later is debt — tenant-specific concerns leak into
the `apps/api` lib and the seam becomes harder to find.

**Self-host packaging excludes `apps/admin`.** A Cargo feature gate
omits the binary from self-host build artifacts; the Helm chart and
docker-compose include `apps/api` only.

## Consequences

**Positive.** Names say what they are. Compiler enforces layering — domain
crates cannot accidentally couple to Axum. Fast incremental builds on the
ARM CI. One binary, two modes, no fork. The tree matches the docs.

**Negative.** Longer crate names at use-sites; `use` aliases mitigate. Six
library manifests instead of one. The lib+bin-in-one-crate pattern is a
v1-only shape: see §F for the extraction triggered by `apps/admin` landing.

**Neutral.** Web and mobile are in the workspace repo but not the Cargo
workspace — independent CI jobs. The common `flight-academy-` prefix is
cosmetic noise in tooling output.

## Alternatives considered

- **Keep the `fa-` prefix.** Less churn but ambiguous; the archive is being
  rewritten anyway so there is no migration cost.
- **Single monolithic crate.** Simpler but enforces no layering and slows
  incremental rebuilds — both worse on the ARM budget.
- **One crate per integration provider.** Multiplies manifests without buying
  isolation; Cargo features (`stripe`, `xero`, …) already gate compilation
  per ADR-001 §E.
- **Separate `flight-academy-api` lib + thin `apps/api` bin.** Common Rust
  pattern. The lib+bin form gives the same testability without a sixth
  ceremony-heavy crate. The proper trigger to add a sixth crate
  (`flight-academy-http-core`) is the second HTTP-speaking binary —
  see §F.

## References

- [ADR-001 §C/§D/§E/§F](ADR-001-platform.md) — the `fa-*` names reconciled in §B.
- [ADR-002 §A/§F/§H](ADR-002-release-deployment.md) — two-repo boundary; DB roles; embedded-static.
- [ADR-003](ADR-003-db-migrations.md) — migrations / schema / `.sqlx` cache live in `flight-academy-db`.
- [ADR-006 §A](ADR-006-api-contract.md) — the emitted spec path consumed by E's client generation.
- [ADR-010 §A](ADR-010-platform-operator-access.md) — internal staff admin is a separate deployable, also under `apps/` when built.
- [docs/design/domain-model.md](../design/domain-model.md) — bounded contexts these crates serve.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 35–36 (restraint on crate count); 28 (truth: the rename is reconciled, not left to drift).

## Notes

The `fa-*` → `flight-academy-*` table in §B is load-bearing: it is how a
contributor reads `fa-db` in ADR-003 as `flight-academy-db` on disk. If a
future ADR adds a crate or touches an earlier ADR's name, it extends this
table.
