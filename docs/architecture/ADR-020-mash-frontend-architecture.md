# ADR-020 — Frontend architecture: Maud + HTMX + Alpine + Tailwind on Axum, CloudFront edge — supersedes ADR-014

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-08 |
| **Deciders** | @ICreateThunder |
| **Tags** | frontend, ssr, no-js, security, performance, deployment, htmx, maud |
| **Supersedes** | [ADR-014](ADR-014-frontend-architecture.md) — frontend architecture |
| **Refines** | [ADR-001 §F](ADR-001-platform.md), [ADR-002 §H](ADR-002-release-deployment.md), [ADR-005](ADR-005-workspace-layout.md), [ADR-015 §B/§C/§E](ADR-015-csp-static-build.md), [ADR-019 §E](ADR-019-white-label-runtime.md) |

## Context

[ADR-014](ADR-014-frontend-architecture.md) selected SvelteKit + `adapter-static` as the web tenant frontend. Three forces have surfaced since that decision and shifted the calculus:

**1. Production constraints have sharpened.** Deployment targets are small-memory ARM nodes (~2GB RAM, 2 vCPU) on K3s. Adding a Node sidecar for SvelteKit `adapter-node` SSR doubles per-pod memory (200-400MB total vs ~50MB for Rust alone) and meaningfully reduces tenant density per node. The aviation safety surface ([ADR-001 §G](ADR-001-platform.md) MOR submission, AOG / ADD marking, dispatch release) must work without JavaScript — a no-JS-degraded experience is unacceptable for safety-critical reporting, regardless of operator's browser environment.

**2. The Rust WASM landscape changed materially.** As of May 2026, Leptos's primary maintainer (Greg Johnston) declared the project *"lightly maintained going forward"* with no successor (issue #4707). Single-maintainer-stepping-back is structurally incompatible with a 5-year aviation platform's regulatory and security horizon. Yew remains mature but slow-cadence and bundle-heavy. Only Dioxus has the commercial backing (Dioxus Labs, YC S23) and aviation-credible adoption (Airbus + ESA collision-avoidance system) to be a 5-year viable Rust WASM bet, but Dioxus is still 0.x with API churn and its component ecosystem (dioxus-primitives, dioxus-components) is too nascent to commit a regulated platform to today.

**3. The shape of the application is CRUD-dominant.** Aviation operations workflows — bookings, logbooks, dispatch boards, MOR forms, work orders, maintenance schedules, currency tracking — are forms, tables, lists and filtered queries. The features that need SPA-style rich client state (drag-and-drop, live brand preview, real-time canvases) are a small minority of surfaces. A framework whose primary value proposition is "complex client-side state management" is paying ongoing complexity tax for a shape we don't have.

The MASH pattern (Maud + Axum + SQLx + HTMX) is the Rust-native expression of the Rails Hotwire / Phoenix LiveView / Django + HTMX approach — server-rendered HTML with incremental enhancement via HTMX attributes. It has been industry-proven at scale across Rails, Django, Phoenix, and Laravel deployments for over a decade. The Rust components (Axum, Maud, SQLx) are mature post-1.0 libraries; HTMX is at 2.x with very slow breaking-change cadence; Alpine.js is at 3.x with active commercial-funded maintenance. None of the load-bearing components carries the single-maintainer-stepping-back risk that disqualified Leptos, nor the 0.x churn risk that disqualifies Dioxus today.

The integration-first JSON API ([ADR-008](ADR-008-data-sharing-posture.md), [ADR-006](ADR-006-api-contract.md), [ADR-018](ADR-018-openapi-emission-format.md)) is independent of the web rendering choice. Flutter mobile, third-party integrations, and automation consume `/api/v1/*`; the web surface renders HTML; both are parallel consumers of business logic in `flight-academy-*` crates. The MASH decision affects only how the web surface renders.

## Decision

**Web tenant surface rendering moves to Maud (compile-time HTML in Rust) + HTMX (server interactions) + Alpine.js (client state) + Tailwind v4 (CSS), served by `apps/api` from the same Rust binary that owns the JSON API. CloudFront edge caches prerendered HTML and static assets via ACM-managed wildcard cert; per-tenant subdomain routing reads the Host header at origin per [ADR-019](ADR-019-white-label-runtime.md). Aviation safety surfaces (MOR submission, AOG marking, ADD entry, dispatch release, magic-link sign-in) MUST work no-JS; the entire application is built to the same no-JS floor wherever the implementation cost is not disproportionate.**

### A. API surface preservation — JSON contract unchanged

`apps/api` continues to expose the JSON API at `/api/v1/*` per [ADR-006](ADR-006-api-contract.md), [ADR-008](ADR-008-data-sharing-posture.md), and [ADR-018](ADR-018-openapi-emission-format.md). Same path patterns, same problem+json errors, same OpenAPI emission to `docs/api/openapi.json`. The MASH decision *adds* an HTML surface to the same binary; it does *not* remove or modify the JSON surface.

**Consumers of `/api/v1/*` after this ADR:**

- `apps/mobile` Flutter client via the Dart wrapped client ([ADR-014 §E](ADR-014-frontend-architecture.md) mobile path preserved)
- Third-party integrations per [ADR-008](ADR-008-data-sharing-posture.md) integration-first posture
- Future automation, webhooks, AI tooling
- Platform monitoring / observability

**The web tenant surface (`/*`)** is server-rendered HTML. There is no JSON-fetching from the browser, no generated TypeScript client. Business logic invariants — ABAC ([ADR-001 §C](ADR-001-platform.md)), audit ([ADR-009](ADR-009-event-streams-and-retention.md)), tenancy gates ([ADR-001 §D](ADR-001-platform.md)) — are upheld by the same code paths the JSON handlers invoke; the web surface inherits them without re-implementation.

### B. Stack — Maud + HTMX + Alpine + Tailwind

| Component | Role | Version baseline |
| --- | --- | --- |
| **Maud** | Compile-time HTML in Rust; type-safe templates colocated with handlers | 0.27+ |
| **Axum** | HTTP server (already in use per [ADR-006](ADR-006-api-contract.md)) | 0.8+ |
| **HTMX 2.x** | Server interactions: `hx-get`, `hx-post`, `hx-boost`, `hx-target`, fragment swaps | 2.x |
| **HTMX extensions** | `sse` (real-time push), `response-targets` (error routing), `preload` (hover-prefetch) | matching HTMX major |
| **Alpine.js 3.x** | Client-only UI state: dropdowns, modals, tabs, form-state reactivity | 3.x |
| **Tailwind v4** | CSS via tight `@theme` mapping to our design tokens | 4.x |
| **Native HTML** | `<dialog>`, `<details>`, `<datalist>`, `popover` attribute used wherever fitness allows — reduces JS surface | universal browser support |

The JS layer (HTMX + extensions + Alpine) totals ~35-40KB gzipped, fully cached after first visit. No SPA framework, no VDOM, no compile-to-WASM runtime. **No Svelte islands** — for genuinely SPA-shaped interactive surfaces (rare in our shape), a vanilla JS Custom Element or Lit-based Web Component is the escape hatch; Svelte tooling is not pulled into the build pipeline at v1.

### C. Static / dynamic surface split

Three buckets, served by the same `apps/api` binary:

| Bucket | Examples | Mechanism | CDN behaviour |
| --- | --- | --- | --- |
| **Prerendered** | Marketing, terms, privacy, help docs, auth landing | Maud template rendered at build time → static `.html` files | Edge-cached at CloudFront long-TTL |
| **Server-rendered (dynamic)** | Dashboard, logbook, bookings, MOR queue, work orders, settings | Axum handler reads request, calls business logic, renders Maud template with tenant/user data | Origin always; CloudFront pass-through |
| **Real-time** | Dispatch board, ramp board, notification feed, brand-editor preview | Initial server-render + SSE / WebSocket pushing HTML fragments HTMX swaps in | Pass-through; CloudFront SSE-aware mode |

The same Maud template modules render both initial state and subsequent HTMX-swap fragments. A handler returns a full page on initial load and a fragment on `HX-Request` header presence; Maud's compile-time HTML composition makes this natural.

### D. Maud template organisation

Per-resource view modules colocated with handlers. Each handler module owns its rendering:

```text
apps/api/src/handlers/
├── tenants/
│   ├── mod.rs       — JSON + HTML handlers
│   └── view.rs      — Maud templates for tenant pages
├── bookings/
│   ├── mod.rs
│   └── view.rs
├── auth/
│   ├── mod.rs
│   └── view.rs
└── shared/
    ├── chrome.rs    — layout, header, sidebar, footer
    └── icons.rs     — SVG icon definitions
```

Templates compile to Rust functions returning Maud's `Markup` type. Type-safety extends to template parameters; refactors at the schema layer surface as compile errors at the template layer. Tailwind v4's content scanner is configured to read `**/*.rs` to extract used classes.

### E. CSS strategy

Tailwind v4 with a tight `@theme` derived from `apps/web-ui/tokens/tokens.json` (kept as the design source-of-truth). The existing `tokens.css` (CSS custom properties) and `base.css` (typography utilities + IBM Plex font setup + first-class dark mode via `light-dark()` per [ADR-014 §B](ADR-014-frontend-architecture.md)) are preserved as Tailwind layer imports.

CSS minimisation tactics applied:

- `@theme` defines only the colours, fonts, spacing scale we use — drops unused defaults
- 3 breakpoints, not 5
- Colour palette limited to design tokens + 3 neutrals
- Skip Tailwind plugins not in use (typography, aspect-ratio plugins evaluated per-need)
- `@layer components` for ~20 common patterns (button-primary, card, input, etc.)

Realistic compiled CSS size: 8-15KB minified for the full application surface.

### F. JavaScript layer

| Library | Role | Bundle | Notes |
| --- | --- | --- | --- |
| HTMX 2.x | Server interaction orchestrator | ~14KB gzipped | Vendored at pinned version |
| HTMX `sse` extension | Server-Sent Events client | ~2KB | For real-time |
| HTMX `response-targets` | Error/success routing to specific DOM targets | ~1KB | |
| HTMX `preload` | Hover prefetch of link targets | ~1KB | |
| Alpine.js 3.x | Client-only UI state | ~15KB | Declarative HTML attributes |
| Native HTML | `<dialog>`, `<details>`, `<datalist>`, `popover` | 0KB | Where fitness allows |
| Lit (Web Components) | Future escape hatch for complex reusable widgets | ~10KB (only when used) | Not used at v1 |

**Total baseline client JS: ~33-35KB gzipped.** With Brotli compression at CloudFront edge, ~20-25KB over the wire. Loaded once per cache lifetime (content-hashed URLs, `max-age=31536000, immutable`); zero bytes on every subsequent visit.

**WASM escape hatch for targeted compute / rendering surfaces.** For future surfaces that are genuinely compute-heavy or visually rich (live flight tracking dashboards with vector chart rendering and real-time positions; in-browser weight-balance calculators where interactive what-if analysis benefits from zero API round-trip latency; METAR / TAF decoder visualisations; VFR route planners with airspace-overlay computation), a targeted Rust → WebAssembly widget can be embedded on the specific page that needs it. Pattern: a focused Rust crate compiled via `wasm-pack`; vendored as a hashed static asset; embedded via `<script type="module" src="/assets/widget-{name}-{hash}.js">` on the host page; initialised against a designated DOM target; consumes JSON from `/api/v1/*` for data; renders into its mounted DOM region. No framework commitment ramifies from using this pattern — it is a per-widget tool, not an architectural change.

For mobile-shaped in-flight information surfaces (live tracking, EFB-style instrument displays, GPS-tracked navigation), the primary path remains [`apps/mobile`](ADR-014-frontend-architecture.md) Flutter — its offline-first posture, native sensor access, background sync, and platform integration are wrong-shaped for the web. The web WASM-widget escape hatch is reserved for desktop / tablet web users who need a subset of computational capability without installing the native app — the minority case for in-flight surfaces specifically, but realistic for office-based use of weight-balance, performance charts, and route planning tooling.

Composition with this ADR: WASM widgets sit under the same nonce-CSP surface as Alpine-driven inline content ([ADR-015 §C](ADR-015-csp-static-build.md)); the embedding HTML page is rendered by Maud; HTMX orchestrates server interactions outside the widget bounds; the widget's own client behaviour is encapsulated to its mounted region. Each widget pulled into scope warrants its own implementation slice and a brief design note in its PR — not a new ADR unless the pattern itself needs revision.

### G. Real-time architecture — SSE default, WebSocket reserved

| Mechanism | Direction | Use cases |
| --- | --- | --- |
| **Server-Sent Events (SSE)** | One-way server → client | Default for all real-time. Booking arrived, AOG declared, MOR submitted, brand updated, dispatch board state change |
| **WebSocket** | Bidirectional | Reserved for genuine bidirectional needs (none anticipated at v1; revisit if collaborative editing or live signalling emerges) |

SSE is built on standard HTTP/1.1; works through any HTTP-aware proxy / CDN with zero special configuration; HTMX's `sse` extension provides first-class client wiring (`hx-ext="sse"`, `sse-connect`, `sse-swap`); auto-reconnects with `Last-Event-ID` resume. Server side: tokio broadcast channels per tenant; each connected client subscribes; server renders an HTML fragment per event and emits it; HTMX swaps the fragment into the right DOM position.

For aviation workflows the directionality is almost entirely server-push (status changes flow outward to dashboards). SSE covers it. WebSocket is reached for only when a feature explicitly needs bidirectional state — no anticipated feature requires it at v1.

### H. CloudFront edge architecture

- **CloudFront distribution** in front of all tenant traffic
- **ACM-managed wildcard cert** `*.flight-academy.app` — covers all tenant subdomains; private key never leaves AWS; auto-rotation
- **AWS WAF** in front of CloudFront — managed rule groups for common attack patterns; aviation-specific rate limits per tenant per route class
- **S3 origin** for static prerendered HTML + assets (CSS, JS, fonts, images)
- **`apps/api` origin** for dynamic SSR pages (in private VPC; CloudFront accesses via origin shield + shared-secret header to prevent CDN-bypass)
- **HTTP/3 / QUIC enabled** at edge
- **103 Early Hints** forwarded from origin

**Per-tenant subdomain routing security properties.** Tenant resolution from `Host` header at `apps/api` is industry-standard (Cloudflare, Stripe, GitLab all use this pattern):

- TLS termination at CloudFront with ACM wildcard cert; private key never leaves AWS infrastructure
- CloudFront sets the `Host` header from the validated TLS SNI handshake; SNI is in the signed ClientHello so attackers cannot forge it on the wire
- Origin requires a CloudFront-shared-secret header on every request — direct origin access (CDN bypass) is rejected, so an attacker who finds the origin IP cannot forge `Host` arbitrarily
- Defence-in-depth on tenant resolution: the `Host` header gives the tenant; the JWT `tenant` claim in the session cookie must match per [ADR-001 §F](ADR-001-platform.md); mismatch returns 403. Even if `Host` were somehow forged, the JWT mismatch blocks cross-tenant access
- Tight DNS hygiene — no dangling CNAME records (subdomain takeover prevention)
- Wildcard cert is a single point of trust; compromise affects all tenants. Mitigated by ACM and CloudFront-managed TLS; per-tenant TLS isolation is the deferred custom-domain feature

### I. Cache strategy per path pattern

| Path pattern | `Cache-Control` | Edge cache | Rationale |
| --- | --- | --- | --- |
| `/assets/{hash}.{css,js,woff2,png,avif}` | `public, max-age=31536000, immutable` | 1 year | Content-hashed; immutable URLs |
| `/static/*.html` (prerendered) | `public, s-maxage=3600, stale-while-revalidate=86400` | 1h edge + SWR | Marketing surface; rare changes |
| `/auth/sign-in` (prerendered) | `public, s-maxage=300` | 5 min edge | Stable landing |
| `/api/v1/tenant/brand.css` ([ADR-019](ADR-019-white-label-runtime.md) endpoint) | Redirect: `max-age=60, must-revalidate`; target: `immutable` | Short on redirect, long on hashed target | Brand updates propagate within ~60s |
| `/dashboard`, `/logbook`, `/bookings/*` (per-user) | `private, no-cache` | Not cached | Cookies; authenticated |
| `/htmx-fragments/*` (per-user HTMX swap endpoints) | `private, no-cache` | Not cached | User-specific |
| `/sse/*` (real-time streams) | `no-store` | Pass-through | Long-lived |

### J. Performance posture — best-in-class commitments

Locked sub-decisions to make the architecture deliver "perceived instant" navigation across the application:

1. **HTMX prefetch on hover** — `data-preload` on links via HTMX `preload` extension; hover with 70ms timeout triggers prefetch; by click time the next page is in browser cache
2. **HTTP `103 Early Hints` from apps/api** — for the brand asset preload ([ADR-019 §B](ADR-019-white-label-runtime.md)) and critical CSS/JS bundles
3. **Per-route `Cache-Control` discipline** — every SSR handler explicitly emits cache headers (table above)
4. **ETag + conditional GET** on SSR HTML — server hashes response body; browser revalidates with `If-None-Match`; 304 cuts response to ~0 bytes
5. **`stale-while-revalidate`** for tenant-shared content (settings pages, prerendered shells) — fresh-enough quickly, refreshed in background
6. **In-process tenant data cache** — tokio `OnceCell` keyed by tenant for brand/settings; invalidated on `tenant.updated` events
7. **Streaming HTML responses** for slow-data pages — initial chrome streams immediately, slow data fragments stream as Maud renders them
8. **Server-Timing headers** in dev/staging — `Server-Timing: route;dur=2, db;dur=18, render;dur=3`
9. **Tailwind compiled CSS purged to used classes only** — 8-15KB realistic
10. **Performance budget as CI gate** — Lighthouse on key routes; FCP / LCP / TTI thresholds; fail PR on regression

Realistic per-page numbers with the CloudFront edge in front:

| Page type | TTFB (warm) | TTI | Notes |
| --- | --- | --- | --- |
| Prerendered (marketing, auth landing) | 30-80ms global | 80-150ms | Edge-cached anywhere |
| SSR per-user (dashboard, logbook) from EU | 80-150ms | 150-300ms | Origin in eu-west-2 |
| SSR per-user from US/Asia | 150-300ms | 250-400ms | Single-region origin; revisit with multi-region |
| HTMX swap navigation (warm) | 30-80ms | feels instant | Fragment only; prefetch covered |

### K. CSP composition per surface

Three surfaces persist from [ADR-015](ADR-015-csp-static-build.md); their distribution shifts:

- **Prerendered routes** → hash-based CSP per [ADR-015 §B](ADR-015-csp-static-build.md). Marketing surface, auth landing, terms. Build-time hash extraction script walks the prerendered HTML and emits the hash list.
- **SSR routes** → hash-based CSP for handlers without per-request inline content. Most app pages.
- **Sensitive routes** → per-request nonce CSP per [ADR-015 §C](ADR-015-csp-static-build.md). Login flow, magic-link verify, OAuth consent ([ADR-011](ADR-011-user-consent-grant.md)), passkey ceremony, brand editor (CSSOM mutation needs an inline `<script>` per [ADR-019 §E](ADR-019-white-label-runtime.md) refined below).
- [ADR-015 §E](ADR-015-csp-static-build.md) inline-style attribute denial preserved — HTMX uses `hx-*` attributes (not `style=`), and Alpine uses `x-*` attributes. Both compose with §E without weakening.

### L. No-JS posture — native to the model

The MASH stack is no-JS first by construction:

- All read paths server-rendered HTML — usable no-JS without any special engineering
- All forms POST natively — `<form method="POST" action="...">` works whether or not HTMX is loaded; HTMX intercepts and AJAX-POSTs only when present
- All links navigate natively — `<a href>` works; HTMX `hx-boost` intercepts for AJAX-swap when present
- Magic-link auth ([ADR-001 §F](ADR-001-platform.md)) — pure HTML flow

**Intrinsically-JS features and their no-JS fallbacks:**

| Feature | No-JS fallback |
| --- | --- |
| WebAuthn / passkey ceremony | Magic-link sign-in (universal floor) |
| Push-notification "approve sign-in" observation | Magic-link or "we sent a push; refresh this page after approving" with refresh button |
| Brand editor live preview ([ADR-019 §E](ADR-019-white-label-runtime.md), refined below) | Save-and-reload form-submit |
| Drag-and-drop reorder | Up/down buttons posting `move_up` / `move_down` form actions |
| Real-time dispatch / ramp updates | Manual refresh button; server re-renders fresh state |
| Optimistic UI / instant feedback | Form-submit with `redirect(303)` and brief loading state |

[ADR-001 §F](ADR-001-platform.md) is refined: magic-link is the universal authentication floor; passkey and push are JS-enhanced alternatives offered when the runtime supports them, never the only path.

**Aviation safety carve-outs — no-JS MANDATORY (CI-enforced):**

- **MOR submission** ([ADR-001 §G](ADR-001-platform.md) safety reporting) — full flow including attachments via plain `<input type="file">` and the anonymisation toggle as a plain checkbox
- **AOG marking** — declaring an aircraft Aircraft on Ground
- **ADD entry** — adding an Acceptable Deferred Defect
- **Dispatch board "release for flight"** — instructor / dispatcher sign-off
- **Magic-link sign-in** — the auth floor itself
- **Regulatory document chrome** ([ADR-019 §F](ADR-019-white-label-runtime.md)) — additionally locked to render no-JS

These surfaces and their fallback paths are exercised in the CI no-JS verification set (§N) and may not regress.

### M. ADR-019 §E refinement — brand editor preview mechanism

[ADR-019 §E](ADR-019-white-label-runtime.md) specified the brand editor live preview as "JS-driven CSSOM mutation on a per-request nonce-CSP sensitive route." Under MASH this becomes specifically **Alpine.js-driven CSSOM mutation** — the editor route uses Alpine's `x-data` and `x-on:input` to bind colour picker changes to `document.documentElement.style.setProperty('--color-brand', value)`. The mechanism remains session-local, mock-pane internal; no Svelte island is required. The nonce-CSP surface from [ADR-015 §C](ADR-015-csp-static-build.md) accommodates Alpine's inline directives via the nonce.

If JavaScript is disabled, the editor form still works: it POSTs to the save handler; the handler updates `tenants.settings.brand` JSONB, computes the new content-hashed CSS asset, redirects via 303 to the editor view; the editor view re-renders with the new tokens applied. Save-and-reload UX rather than live preview.

### N. CI verification

Two cheap gates enforce the architectural contracts at PR time:

1. **OpenAPI diff must be empty.** The mobile path and third-party integrations consume `/api/v1/*`; the migration must not silently break the JSON contract. CI emits the OpenAPI spec, diffs against `docs/api/openapi.json`, fails on any unintended change.
2. **No-JS-critical route smoke test.** Playwright with `javaScriptEnabled: false` walks the named safety-critical route set (auth flow, MOR submission, AOG / ADD / dispatch, primary read paths). Each assertion: page returns 200; meaningful content renders; critical form controls present; on form submit, the next page renders with the expected state change.
3. **Performance budget Lighthouse** on a known-route set; fails PR on FCP / LCP / TTI regression beyond a tolerance.
4. **CSS bundle size** asserted under 20KB (gzipped) — early warning on Tailwind config drift.

### O. Build pipeline

Build flow:

1. Bun installs Tailwind CLI (dev dependency only; not in production runtime)
2. `apps/api/build.rs` runs Tailwind compile → `apps/api/static/app.{hash}.css`
3. Vendored HTMX + extensions + Alpine + IBM Plex woff2 are copied to `apps/api/static/`
4. Self-hosted IBM Plex fonts subsetted to Latin + Latin Extended (existing from prior work)
5. Cargo builds `apps/api` — Maud templates compile into the binary as Rust code

Hosted vs self-host distribution:

| Variant | Cargo feature | Static asset serving |
| --- | --- | --- |
| **Hosted production** | (default, no embedded-static) | S3 → CloudFront edge serves all `/assets/*` and prerendered `/static/*.html`; `apps/api` serves dynamic routes only |
| **Self-host single binary** | `--features embedded-static` | `rust-embed` bakes `apps/api/static/` into the binary; single binary serves everything from memory |

This is the original [ADR-002 §H](ADR-002-release-deployment.md) single-binary posture *returning* after the proposed SvelteKit `adapter-node` ADR-020-draft would have broken it. Two artefacts published per release: `flight-academy` (hosted; S3 assets) and `flight-academy-embedded` (self-host; embedded assets).

### P. Workspace layout changes

**Deleted:**

- `apps/web/` — SvelteKit skeleton
- `apps/web-ui/styles/`, `apps/web-ui/scripts/`, `apps/web-ui/package.json` — Svelte-specific bits
- Root `package.json` workspace config — replaced with minimal Tailwind dev config

**Preserved:**

- `apps/web-ui/tokens/tokens.json` — design tokens source-of-truth; consumed by Tailwind `@theme`
- `apps/web-ui/tokens/tokens.schema.json` — IDE validation
- `apps/web-ui/tokens/tokens.css` — emitted CSS custom properties, imported into Tailwind layer

**Added:**

- `apps/api/src/handlers/<resource>/view.rs` — Maud template modules per resource
- `apps/api/src/handlers/shared/{chrome,icons}.rs` — layout + icon helpers
- `apps/api/static/` — vendored HTMX, Alpine, IBM Plex woff2, compiled Tailwind CSS
- `apps/api/build.rs` — Tailwind compile orchestration + asset hashing
- `apps/api/Cargo.toml` — `maud`, optional `rust-embed` (under `embedded-static` feature)

### Q. Migration plan

1. **ADR-020 lands** (this PR) — architectural decision committed; no code changes yet
2. **Workspace pruning** — delete SvelteKit `apps/web`, prune `apps/web-ui` to tokens-only; update Cargo workspace; update CI `Web CI` workflow to MASH equivalent
3. **MASH foundations** — add `maud` dependency; first Maud handler (e.g. `/healthz` as HTML); vendor HTMX + Alpine; Tailwind compile in `build.rs`
4. **First feature surface** — port a single end-to-end flow (e.g. tenant GET as both JSON and HTML) to validate the end-to-end pattern
5. **Subsequent slices** — each feature lands its JSON + HTML handlers + view module together
6. **CI invariant throughout** — OpenAPI diff must be empty; no-JS verification grows as routes are added

## Consequences

### Positive

- **Smallest production attack surface of any option considered.** Single Rust binary; no Node, no npm in production runtime; no V8 engine. The full supply chain is Cargo-managed under existing `cargo-deny` + `cargo-audit` gates.
- **Smallest memory footprint.** 30-80MB per pod baseline (vs ~200-400MB for SvelteKit `adapter-node`, ~100MB for Dioxus). Materially better tenant density on ARM small-memory nodes.
- **Highest perceived performance with CloudFront in front.** Sub-100ms TTFB for cached content globally; sub-200ms for per-user dynamic content (EU); HTMX prefetch makes subsequent navigation feel instant.
- **Native no-JS support across the entire surface.** Aviation safety surfaces are not a special accommodation — they fall out of the architecture for free. The CI floor for safety-critical routes is enforced without requiring per-route engineering attention.
- **Single binary deployment returns** ([ADR-002 §H](ADR-002-release-deployment.md) original posture). Self-host is one container; one process; one Postgres + one MinIO + the embedded binary.
- **5-year stability across all load-bearing components.** Axum (post-1.0, Tokio-backed), Maud (long-stable API), HTMX 2.x (slow breaking-change cadence), Alpine 3.x (active commercial maintenance), Tailwind v4 (Vercel-backed). No 0.x bets, no single-maintainer-stepping-back risks.
- **Hybrid static / dynamic rendering is native to the model.** Prerendered marketing pages, server-rendered app pages, and HTMX-enhanced interactive surfaces are all expressed in the same Maud template language and the same Axum routing.
- **OpenAPI integration surface preserved.** Mobile path, third-party integrations, automation — all unchanged. The Flutter app lands against the same `/api/v1/*` contract.
- **No client-side state management complexity.** The application is server-as-source-of-truth; HTMX swaps fragments rendered by the server; the entire class of "client and server disagreed about what the page shows" bugs is structurally precluded.
- **Easier audit and compliance.** Single Rust binary; single language; single supply chain. The audit story is materially simpler for ISO 27001 / Cyber Essentials Plus posture ([ADR-016 §C](ADR-016-compliance-baseline.md)).

### Negative

- **SvelteKit scaffolding discarded** — six commits of `apps/web` setup, design token integration, base styles, CI workflow are thrown away. The design tokens themselves (`tokens.json`, `base.css` CSS values) are preserved.
- **No SPA-style rich client state.** HTMX pattern is "server orchestrates state; fragment swaps update the UI." For features that genuinely need rich client state (complex datepickers, drag-and-drop interfaces, real-time canvases), the escape hatch is a Lit-based Web Component or vanilla JS Custom Element — more work than dropping in a Svelte component.
- **MASH developer mental model.** Differs from React/Svelte/Vue patterns most contemporary frontend developers know. Hiring pool is smaller; ramp-up is non-trivial. Mitigated by Rails Hotwire's widespread adoption — the pattern is documented and the LLM-tooling corpus is large.
- **Single-region origin TTFB varies with geography.** Per-user dynamic content from Asia / US sees 200-400ms TTFB cold. Mitigated by HTMX prefetch (subsequent navigation feels instant) and by deferred multi-region origin (revisit if tenants in distant geographies need lower TTFB).
- **Tooling ecosystem around MASH is less polished** than around SvelteKit. No `bits-ui` equivalent; complex UI primitives may need bespoke work. Mitigated by aggressive use of native HTML elements (`<dialog>`, `<details>`, `<datalist>`, `popover`) and by Tailwind v4's component-library ecosystem (Tailwind UI, DaisyUI, Pico if needed).
- **No CDN-edge SSR.** Dynamic SSR runs at origin only. Cloudflare Workers / edge functions would enable sub-50ms TTFB globally for dynamic content but require WASM rendering at edge. Out of v1; defer until measurably needed.

### Neutral

- The brand editor preview mechanism shifts from generic JS CSSOM to Alpine-driven CSSOM ([ADR-019 §E](ADR-019-white-label-runtime.md) refinement). Same nonce-CSP surface; same session-local behaviour; same save-and-reload no-JS fallback.
- Per-tenant subdomain routing through CloudFront wildcard cert is industry-standard with documented security properties (§H).
- Custom domain support is deferred per [ADR-019](ADR-019-white-label-runtime.md); not in scope for ADR-020.
- HTTP/2 multiplexing at CloudFront edge is enabled by default; HTTP/3 / QUIC where supported.

## Alternatives considered

### Alternative A — SvelteKit + `adapter-node` SSR (previous ADR-020 draft)

SvelteKit moves from `adapter-static` to `adapter-node`; SSR every dynamic page on a Node sidecar; `apps/api` Rust fronts and reverse-proxies HTML to Node on localhost.

Rejected. Two-process deployment per pod doubles memory baseline; Node + npm in production adds an attack-surface dimension that requires ongoing supply-chain vigilance; deployment topology fragmentation (single binary becomes two-component) reverses the [ADR-002 §H](ADR-002-release-deployment.md) appeal. The benefits SvelteKit brings (component library ecosystem, mature progressive enhancement story) are partially offset by the maturity of HTMX patterns and native HTML elements in 2026; the costs are not.

### Alternative B — Dioxus (Rust WASM with commercial backing)

Rust WASM frontend; SSR + WASM client; single Rust binary serves both; commercial backing via Dioxus Labs (YC S23); aviation-credible production deployments (Airbus + ESA collision-avoidance system).

Rejected. The 0.x maturity is the *secondary* reason; the primary reason is that **MASH wins on every performance and resource dimension that matters for our shape regardless of Dioxus's maturity**:

| Dimension | Dioxus 0.7 | MASH | Net |
| --- | --- | --- | --- |
| Client bundle baseline | 100-250KB gzipped WASM | ~35KB gzipped JS | MASH 3-5× smaller |
| Client hydration cost | 50-200ms WASM compile + instantiate | None (HTMX event bindings <10ms) | MASH wins |
| Server SSR latency | 2-10ms | 1-5ms | MASH slightly faster |
| Server memory baseline | 50-150MB | 30-50MB | MASH ~2× smaller |
| Per-request memory | 10-50KB | 2-5KB | MASH ~5× smaller |
| Per-tenant density on 2GB ARM node | 10-20 tenants | 20-50 tenants | MASH 2× headroom |

Dioxus's value proposition — rich SPA-style client state with type-safe reactivity across the wire — is real, but pays ongoing complexity and resource tax for a UI shape we do not have. The aviation operations surface is CRUD-heavy with a small minority of genuinely interactive surfaces, not compute-heavy, not canvas-heavy, not real-time-multiplayer-shaped. The 0.x churn risk would be acceptable if Dioxus were structurally better for our shape; it is not, so the maturity question is moot. Worth re-evaluating only if our requirements shift to genuinely SPA-shaped interactive surfaces in significant volume — at which point the WASM escape hatch (§F) covers the targeted cases without a wholesale framework commitment.

### Alternative C — Leptos (Rust WASM with fine-grained reactivity)

Was the most polished Rust WASM option for SSR + signals; small client bundles; first-class server functions.

Rejected. May 2026 maintainer status update: *"not abandoned but lightly maintained going forward"* and *"feature-complete"* (issue #4707). Single-primary-maintainer-stepping-back is structurally incompatible with a 5-year aviation platform's regulatory horizon. Who lands the security patch at year 3? The answer is not "us, forking the framework and becoming a small framework company" — that maintenance cost is wrong-shaped.

### Alternative D — Yew (mature Rust WASM, VDOM)

Older Rust WASM framework; recent 0.22 release ("For Real This Time") after a two-year gap from 0.21.

Rejected. Cadence is slow; bundle sizes are 3-4× MASH's client baseline; no first-party component library that matches the Radix/shadcn quality bar. Mature but not load-bearing for our needs.

### Alternative E — Islands architecture (Maud HTML + Svelte islands for interactive bits)

Maud renders all HTML; Svelte components embedded as `<script type="module">` islands for genuinely complex interactive surfaces.

Rejected at v1. The Lit / Web Components / native HTML escape hatch covers our anticipated interactive needs without requiring a Svelte build pipeline in scope. May re-evaluate if a future feature emerges where Lit feels inadequate.

### Alternative F — Content negotiation on `/api/v1/*` (Accept header decides JSON vs HTML)

Same URL serves JSON or HTML based on `Accept` header.

Rejected. Conflates integration API (versioned, OpenAPI-documented, [ADR-008](ADR-008-data-sharing-posture.md) integration-first posture) with browser form receiver (cookie-auth, redirect-driven, in-page error rendering). `Accept`-default ambiguity, `Vary: Accept` cache penalty, OpenAPI cleanliness loss. Industry pattern matches separation — Stripe, GitHub, GitLab, Mastodon all keep public APIs and web UIs in distinct URL spaces.

### Alternative G — Maud + Tera/Askama (runtime template engines instead of compile-time)

Tera or Askama (runtime Jinja-style templates) instead of Maud (compile-time HTML in Rust).

Rejected. Maud's compile-time guarantees match the rest of our Rust posture — template refactors at the schema layer surface as compile errors at the template layer; runtime template-string concatenation is denied at compile time; HTML escaping is enforced by the type system. Runtime engines trade compile-time safety for slightly more familiar templating syntax; the trade is not worth it for the safety-critical surfaces in scope.

## References

- Supersedes [ADR-014](ADR-014-frontend-architecture.md) — frontend architecture
- Refines [ADR-001 §F](ADR-001-platform.md) — auth session: magic-link is the universal floor; passkey + push are JS-enhanced alternatives
- Refines [ADR-002 §H](ADR-002-release-deployment.md) — self-host single binary returns; hosted variant uses S3 + CloudFront for asset serving; `embedded-static` cargo feature returns
- Refines [ADR-005](ADR-005-workspace-layout.md) — `apps/web` scaffolding removed; `apps/web-ui` pruned to tokens-only; `apps/api/src/handlers/<resource>/view.rs` added
- Refines [ADR-015 §B/§C/§E](ADR-015-csp-static-build.md) — CSP composition per surface; hash CSP for prerendered + most SSR; nonce CSP for sensitive routes (login, OAuth consent, brand editor); inline-style attribute denial preserved
- Refines [ADR-019 §E](ADR-019-white-label-runtime.md) — brand editor preview mechanism is Alpine-driven CSSOM (not generic JS); save-and-reload no-JS fallback
- Composes with [ADR-006](ADR-006-api-contract.md) — JSON API contract unchanged
- Composes with [ADR-008](ADR-008-data-sharing-posture.md) — integration-first posture preserved
- Composes with [ADR-018](ADR-018-openapi-emission-format.md) — OpenAPI emission unchanged
- Composes with [ADR-017](ADR-017-outbound-http-ssrf.md) — outbound HTTP posture preserved (no outbound from web rendering; `apps/api` is the chokepoint)
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 24 (no pretence: the SvelteKit decision was honestly tested and reversed when constraints sharpened, not silently abandoned), 28 (truth: the aviation safety floor is the immutable constraint), 35–36 (restraint: scope is narrow; no SPA-framework added; intrinsically-JS features have documented fallbacks)
- [Greg Johnston, "Leptos Status Update" (Issue #4707, May 2026)](https://github.com/leptos-rs/leptos/issues/4707) — source of the Leptos lightly-maintained signal that eliminated the Rust WASM alternative for our timeline

## Notes

The "MASH" naming follows the community convention (Maud + Axum + SQLx + HTMX) established in [Building a Fast Website with the MASH Stack in Rust](https://emschwartz.me/building-a-fast-website-with-the-mash-stack-in-rust/). The addition of Alpine.js + Tailwind v4 to the stack is conventional in deployed MASH-shaped projects.

The decision is reversible at the architectural seam: the JSON API contract `/api/v1/*` is unchanged; the web surface rendering is the only thing that swaps. If a future Rust UI framework (Dioxus 1.0, a successor Leptos under new maintainership, or a not-yet-emergent framework) becomes compelling, the migration path is well-understood — Maud handlers become framework component renderers calling the same business logic.

The aviation safety floor (§L) is the immutable contract for this and future frontend ADRs. Any successor frontend architecture must preserve no-JS rendering for MOR, AOG, ADD, dispatch release, and magic-link sign-in; this constraint precedes framework preferences. CI enforcement (§N) is the mechanism that makes the contract real.
