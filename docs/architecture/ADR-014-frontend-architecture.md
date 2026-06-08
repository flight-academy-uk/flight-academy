# ADR-014 — Frontend architecture

| Field | Value |
| --- | --- |
| **Status** | Superseded by [ADR-020](ADR-020-mash-frontend-architecture.md) |
| **Date** | 2026-06-02 |
| **Superseded on** | 2026-06-08 |
| **Deciders** | @ICreateThunder |
| **Tags** | frontend, sveltekit, flutter, white-label, components, embedded-static |
| **Supersedes** | (none — refines [ADR-001 §B](ADR-001-platform.md), [ADR-005](ADR-005-workspace-layout.md)) |
| **Superseded by** | [ADR-020](ADR-020-mash-frontend-architecture.md) — frontend stack changes from SvelteKit to MASH (Maud + Axum + SQLx + HTMX + Alpine + Tailwind v4); the JSON API contract, the Dart mobile client path (§E), the design-token bridge (§C), and the first-party tenant asset pipeline (§F) all carry forward unchanged into ADR-020. The body of this ADR remains as historical record per [GOVERNANCE.md](../../GOVERNANCE.md). |

## Context

[ADR-001 §B](ADR-001-platform.md) established SvelteKit + Tailwind v4
for web and Flutter for mobile. [ADR-005 §A](ADR-005-workspace-layout.md)
placed `apps/web` and `apps/mobile`. [ADR-005 §F](ADR-005-workspace-layout.md)
anticipated `apps/admin` as a future second binary;
[ADR-010 §I](ADR-010-platform-operator-access.md) specified a separate
staff plane web surface. Per-tenant colour and branding customisation
runs via CSS custom properties driven from tenant settings JSONB. The
porting source is roughly 43 React JSX views from the internal design
prototype (committed under `docs/design/`). The web stack is SvelteKit
with Tailwind v4, and `system.css` is reused near-verbatim from the
design source.

Open questions: how many SvelteKit codebases? How are components
shared between web and mobile (different runtimes)? How does the
static build interact with the API binary's embedded-static mode
([ADR-002 §H](ADR-002-release-deployment.md))? How is the generated
TS client ([ADR-005 §E](ADR-005-workspace-layout.md)) wrapped?

Forces: code reuse without forcing impossible cross-runtime sharing;
compile-time separation of tenant and staff surfaces (blast radius,
[ADR-010 §I](ADR-010-platform-operator-access.md)); embedded-static
simplicity (one Rust binary serves both API and SPA in single-binary
mode); white-label runtime (tenant theming applies dynamically without
rebuild); self-host parity (static-only build works without dynamic SSR
infrastructure).

Scope is narrow on purpose: codebase boundary, shared-primitive
strategy, embedded-static handshake, generated-client wrapping.
Tactical choices — state management library, form library, routing
patterns, animation, accessibility tooling — are deliberately out of
scope and land via convention.

## Decision

**Two SvelteKit codebases (`apps/web` tenant-facing; `apps/admin-web`
staff plane) plus one Flutter codebase (`apps/mobile`). A shared
workspace package `apps/web-ui` holds Svelte component primitives and
the design-token JSON. Design tokens are the boundary between web and
mobile — JSON generates CSS custom properties for Svelte and
`ThemeData` for Flutter. The SvelteKit build emits to
`apps/web/build/` and `apps/admin-web/build/`; the API binary's
`embedded-static` feature includes the tenant build only. Generated TS
client is wrapped by a thin module that adds auth, tenant context, and
error normalisation; consumers never touch the generated module
directly.**

### A. Codebase count — two SvelteKit + one Flutter

| Codebase | Surface | Why separate |
| --- | --- | --- |
| `apps/web` | Tenant-facing — pilots, instructors, tenant admins, org admins | The public attack surface and the white-label experience |
| `apps/admin-web` | Staff plane ([ADR-010 §I](ADR-010-platform-operator-access.md)) — platform operators | Compile-time guarantee staff routes are unreachable from the tenant bundle; different auth, theming, dependencies |
| `apps/mobile` | Flutter — pilots primarily, instructors secondarily | Native UX on iOS/Android; SvelteKit cannot deliver native flight-deck UX |

Route-level separation within one SvelteKit codebase was considered.
Rejected for the same reason `apps/api` and `apps/admin` are separate
binaries ([ADR-005 §F](ADR-005-workspace-layout.md)): a bundling bug
or middleware misconfiguration in one bundle is the wrong place to
discover staff routes are reachable to tenant users.

### B. Shared UI library — `apps/web-ui`

A workspace package at `apps/web-ui` (Bun workspace; not a Cargo
crate). **Bun is the preferred package manager and runtime** for the
performance characteristics (fast install, fast test, native TypeScript,
fewer cold-start delays in CI). Fallback to npm or pnpm is permitted
if a contributor hits a core-Node-module incompatibility that Bun's
shim layer does not yet cover; that fallback is implementation-level
and not ADR-bound.

```text
apps/web-ui/
├── package.json
├── src/
│   ├── components/         # Svelte primitives (Button, Input, Modal, …)
│   ├── icons/              # Inline SVG icon set
│   └── lib/                # Pure utilities (formatters, validators)
└── tokens/
    ├── tokens.json         # Source of truth (design tokens)
    ├── tokens.css          # Emitted CSS custom properties
    └── tokens.dart         # Emitted Flutter ThemeData
```

`apps/web` and `apps/admin-web` both declare `apps/web-ui` as a
workspace dependency. `apps/mobile` consumes the emitted `tokens.dart`
but not the components — Svelte primitives don't cross the runtime
boundary.

The component set targets ~9 primitives from the JSX design source,
reimplemented in Svelte. **`bits-ui` is the formally adopted
headless-primitive library** for interactive components (Dialog,
Popover, Combobox, Menu, Select, Tabs) — chosen for keyboard /
focus / ARIA correctness without imposed visual decisions. Layout
primitives are hand-rolled. Out of scope here: which specific
primitives — that's port-time work.

### C. Design tokens as the web↔mobile bridge

`apps/web-ui/tokens/tokens.json` is the single source of truth:

```json
{
  "colour": {
    "brand": { "value": "{tenant.brand}" },
    "surface": { "primary": "oklch(98% 0 0)" }
  },
  "spacing": { "xs": "0.25rem" },
  "radius": { "sm": "0.25rem" },
  "type": {}
}
```

A build script transforms `tokens.json` into:

- `tokens.css` — CSS custom properties consumed by Tailwind v4 (`@theme`
  directive) and `system.css`.
- `tokens.dart` — a Flutter `ThemeData` generator.

Tenant-overridable tokens (those with `{tenant.…}` value markers)
become CSS custom properties at runtime (§F) or per-tenant Flutter
theme instances. The token JSON shape is the contract; the emitters
are derived.

This is the only design coordination point between web and mobile.
Component code stays per-runtime; visual language stays consistent.

### D. Embedded-static build handshake

The single-binary deployment ([ADR-002 §H](ADR-002-release-deployment.md))
includes the pre-built tenant SvelteKit bundle into `apps/api` via
`rust-embed` under the `embedded-static` feature. Build order:

1. Bun installs and builds `apps/web` (with `apps/web-ui` as workspace
   dep). Output: `apps/web/build/`.
2. Cargo builds `apps/api` with `--features embedded-static`;
   `rust-embed` reads `apps/web/build/`.
3. Container build ships the single binary.

The CI pipeline runs steps 1 and 2 in sequence. Cargo's build script
depends on the contents of `apps/web/build/` so incremental rebuilds
work correctly.

The staff plane's SvelteKit build (`apps/admin-web/build/`) is **not**
embedded into either binary. It is served by `apps/admin` as static
files from a sidecar volume or by a dedicated internal ingress. Staff
plane has no single-binary deployment mode — self-host has no staff
plane ([ADR-010 §H](ADR-010-platform-operator-access.md)), and
hosted-mode staff plane runs as its own Deployment.

### E. Generated client wrapping

The TS client generated at `apps/web/src/lib/api/generated/`
([ADR-005 §E](ADR-005-workspace-layout.md)) is wrapped by:

```text
apps/web/src/lib/api/
├── generated/              # gitignored, emitted at build
├── client.ts               # constructs the wrapped client (auth + context)
├── error.ts                # normalises problem+json → typed AppError
└── index.ts                # re-exports the wrapped client + types
```

Application code imports from `$lib/api`, never from `$lib/api/generated`.
The wrapper:

- Reads the session token from cookie / store and injects
  `Authorization: Bearer …`.
- Reads the active tenant id from store and injects via path or header
  per [ADR-006 §C](ADR-006-api-contract.md).
- Injects `X-Client-Version` (build-stamped at compile time) on every
  request per [ADR-006 §J](ADR-006-api-contract.md).
- Catches `problem+json` ([ADR-006](ADR-006-api-contract.md), RFC 9457)
  errors and translates to a typed `AppError` discriminated union.
- Handles `426 Upgrade Required` ([ADR-006 §J](ADR-006-api-contract.md))
  by surfacing a forced-refresh UI; subsequent API calls fail until
  the user reloads. This is the **benign stale client** side of the
  CVE emergency-break mechanism
  ([ADR-015 §I](ADR-015-csp-static-build.md)) — the load-bearing
  active-compromise defence is server-side session-key rotation
  ([ADR-013 §F](ADR-013-auth-keys.md)).
- Surfaces retry/abort via standard `AbortSignal`.

The same pattern applies to `apps/admin-web/src/lib/api/` against the
staff-plane generated client. `apps/mobile`'s generated Dart client is
wrapped equivalently at `apps/mobile/lib/api/` — same shape, different
runtime.

### F. White-label runtime

CSS custom properties for tenant-overridable tokens load at app boot:

1. Bootstrap fetch resolves the active tenant (subdomain, path prefix,
   or default).
2. Tenant settings JSONB provides override values for brand and accent
   colour tokens plus **first-party asset references** for logo and
   custom fonts (asset IDs, never URLs to tenant-controlled origins).
3. A `<style>` block injects `--brand-primary: …` on `:root`.
4. All component styles use `var(--brand-primary)`, never literal
   colour values.

The override set is bounded — typography is constrained, layout is
fixed, only the token-listed properties are tenant-overridable.
White-label means colour and identity, not arbitrary design freedom.

Flutter equivalent: bootstrap fetch returns the tenant's token
override JSON; the app constructs a `ThemeData` from merged base +
overrides at startup.

**First-party hosting of branding assets.** Tenant branding assets
(logos, custom fonts, OG images) are **uploaded to
`flight-academy-store` at tenant configuration time, never referenced
by URL**. Same pattern and rationale as OAuth client logos
([ADR-011 §D](ADR-011-user-consent-grant.md)) — closing SSRF
([ADR-017](ADR-017-outbound-http-ssrf.md)) at the branding surface
while gaining real performance benefits:

- **Transcoding pipeline at upload.** Originals stored in
  `flight-academy-store`. **SVG uploads are served as-is** (vector;
  no transcoding; sanitised against script content). Raster uploads
  (PNG/JPEG/HEIC) produce AVIF and WebP variants plus a JPEG/PNG
  fallback for legacy clients, at standard size tiers (e.g. logo at
  64/128/256/512 px). One-time cost amortised over every request
  thereafter.
- **Content negotiation at serve.** Static handler in `apps/api`
  reads `Accept` and returns the best available format
  (SVG when the asset is vector; otherwise AVIF → WebP → fallback).
  AVIF saves ~50% over JPEG at equivalent quality; WebP ~30%; SVG is
  byte-perfect at any resolution for logos that are vector-shaped.
- **CDN caching.** Hashed-filename URLs (`{asset_id}-{hash}.avif`)
  with long TTL (`max-age=31536000, immutable`); the hash changes on
  re-upload so cache invalidation is automatic.
- **Validation at upload.** MIME type allow-list, max dimensions, max
  file size, content scan — same surface as the OAuth client logo
  upload.
- **Stable URLs and privacy.** No third-party server learns about
  tenant users on every render; no breakage if the tenant's external
  asset host goes down.

The Flutter app fetches the same asset URLs; transcoded variants and
content negotiation work identically.

### G. Self-host

Self-host deployments run `apps/api` only
([ADR-010 §H](ADR-010-platform-operator-access.md)) with the embedded
tenant SvelteKit bundle. `apps/admin-web` is not built in the
self-host CI path. White-label runtime works identically — the
self-hosting tenant has the same tenant settings JSONB and override
surface as any hosted tenant.

Self-hosters needing a custom build of `apps/web` (significant fork)
supply their own `apps/web/build/` and cargo-build with
`embedded-static`. Token schema stability is not guaranteed across
major versions.

## Consequences

**Positive.** Compile-time separation between tenant and staff plane
mirrors the runtime separation
([ADR-010 §I](ADR-010-platform-operator-access.md)). Design tokens
bridge cross-runtime without forcing component-code sharing.
Embedded-static handshake is straightforward, deterministic,
CI-friendly. Generated-client wrapping keeps API ergonomics consistent
and prevents direct generated-module use (which would break at every
spec update). White-label runtime is bounded and predictable.

**Negative.** Two SvelteKit codebases duplicate some build config,
dependencies, and CI steps. Token transformation is an additional
build artefact to maintain. Embedded-static rebuilds the SPA on every
API rebuild — slow without caching. Shared workspace package
coordination between two SvelteKit consumers is real overhead.

**Neutral.** Primitive count (~9) is small enough to maintain. The
JSX → SvelteKit port is a one-time activity covered by other docs.

## Alternatives considered

- **One SvelteKit codebase, route-level separation.** Smaller
  footprint; loses compile-time tenant/staff separation; one bundle
  leak risks the wrong audience seeing staff routes. Rejected on
  blast-radius grounds.
- **Three SvelteKit codebases (tenant + staff + marketing).**
  Marketing site is its own concern (SEO, A/B, CMS); separate
  codebase makes sense but not in this ADR. When a marketing site
  lands, it's its own SvelteKit codebase by extension of this pattern.
- **Component sharing via runtime federation.** Module federation
  between Svelte and Flutter would require runtime bridges that don't
  exist. Rejected on impossibility.
- **No shared package — copy-paste components.** Avoids workspace
  coordination cost; loses single-source-of-truth for primitives.
  Rejected.
- **SSR-by-default instead of static-build.** SvelteKit `adapter-node`
  handles dynamic CSP nonces ([ADR-015](ADR-015-csp-static-build.md))
  uniformly. Rejected: complicates self-host (Node.js sidecar),
  breaks single-binary deployment
  ([ADR-002 §H](ADR-002-release-deployment.md)).
- **Different framework (SolidStart, Astro, Nuxt).** SvelteKit
  chosen in [ADR-001 §B](ADR-001-platform.md); not revisited here.

## References

- [ADR-001 §B](ADR-001-platform.md) — refined here (per-codebase
  shape).
- [ADR-002 §H](ADR-002-release-deployment.md) — embedded-static mode;
  §D handshake.
- [ADR-005 §A/§E/§F](ADR-005-workspace-layout.md) — workspace layout;
  generated client paths; `apps/admin` extraction.
- [ADR-006 §C](ADR-006-api-contract.md) — tenant context; §E wrapping.
- [ADR-010 §I](ADR-010-platform-operator-access.md) — staff plane
  runtime topology; binary separation extends to SvelteKit separation.
- [ADR-015](ADR-015-csp-static-build.md) — CSP / static-build
  reconciliation; how this codebase shape works under CSP.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 28
  (truth — codebase split mirrors the real authorisation boundary;
  design tokens are named contracts not implicit overrides), 34 (be
  not proud — `bits-ui` rather than reinvent; PWA out-of-scope
  rather than over-promised), 35–36 (restraint — ~9 primitives;
  white-label is colour and identity, not arbitrary design freedom;
  out-of-scope list explicit), 38 (be not lazy — accessibility
  tested at primitives and end-to-end; port discipline before
  feature work), 48 (watchfulness — separation extends from binary
  through codebase to CSP; white-label runtime bounded).

## Notes

Explicitly out of scope: state management library (Svelte stores by
default; reach for nanostores if shared state across routes becomes
painful), form library (port the JSX pattern), routing patterns
(SvelteKit conventions), animation, accessibility tooling beyond
WCAG 2.2 AA via component primitives
([ADR-016 §D](ADR-016-compliance-baseline.md)). Each is convention /
PR-review territory.

`apps/web-ui` is a workspace package, not a published npm package. If
the primitives ever ship to other Flight Academy projects or third
parties, an "npm publish" decision is its own ADR.

**PWA / service worker — deliberately out of scope for v1.** Offline
usage is the **Flutter mobile app**'s responsibility, not the web
client's. The decision is deliberate, not an omission:

- The mobile app delivers the offline-first flight-deck UX (logbook
  entry, currency check, lesson record-keeping during a flight where
  network is unavailable).
- PWA capability is **declining** — Manifest v3 restrictions, varied
  cross-browser support, and the historical pattern of PWA-specific
  attack surfaces (insecure caches, service-worker hijack, scope
  pollution) make it a poorer fit for sensitive workflows than a
  native app.
- If a web-side offline need ever emerges that the mobile app cannot
  satisfy, **Flutter can target the web** as an additional output —
  this defers the decision rather than precluding it.

A future ADR addresses PWA support if the need becomes concrete.
SvelteKit's PWA tooling would be the implementation path then, but
the security review would be substantial — see
[ADR-017](ADR-017-outbound-http-ssrf.md) and
[ADR-015](ADR-015-csp-static-build.md) for the relevant constraints.
