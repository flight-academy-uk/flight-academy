# ADR-019 — White-label runtime — refines ADR-014 §F brand injection mechanism

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-08 |
| **Deciders** | @ICreateThunder |
| **Tags** | frontend, white-label, csp, tenants, assets, accessibility |
| **Supersedes** | (none — refines [ADR-014 §F](ADR-014-frontend-architecture.md), clarifies composition with [ADR-015 §B/§C/§E](ADR-015-csp-static-build.md)) |

## Context

[ADR-014 §F](ADR-014-frontend-architecture.md) describes the white-label runtime as a boot-time `<style>` block on `:root` injecting `--brand-*` custom properties from tenant settings JSONB. [ADR-015 §B](ADR-015-csp-static-build.md) requires every inline `<script>` and `<style>` on the static tenant surface to be enumerated by SHA-512 hash in the response CSP, computed at build time. Per-tenant content is only known at request time, so a hash list built against `apps/web/build/` cannot cover a per-tenant inline `<style>`. [ADR-015 §E](ADR-015-csp-static-build.md) independently denies `style="..."` HTML attributes across all surfaces. The literal §F mechanism is incompatible with §B and §E.

The conflict is one instance of a broader pattern: the static surface references several per-tenant artefacts the build pipeline cannot inline. Tenant logo, custom fonts, OG image and favicon all share the same problem — the static HTML must reference them, but their bytes are only known after the tenant configures them. ADR-014 §F already names a first-party asset pipeline for the uploaded artefacts; brand CSS sits structurally alongside them as **derived** content that an emitter produces from JSONB rather than the tenant uploading.

Two further forces shape the answer. Aviation status colours (success / warning / error / info) and regulatory document chrome (CAA paperwork, MOR submissions, examiner audit packs) communicate safety severity uniformly across tenants — an instructor moving between operators must read the same colours the same way. This is a safety, not aesthetic, surface, and is the explicit reason the white-label scope here is narrower than a typical SaaS theming feature. Second, accessibility: a tenant choosing a brand colour that fails WCAG AA contrast against neutral surfaces will silently degrade every user's experience and put us below the floor [ADR-016 §C](ADR-016-compliance-baseline.md) commits to.

The brand editor is itself a per-tenant authenticated route under [ADR-015 §C](ADR-015-csp-static-build.md) nonce policy — the preview mechanism rides on the existing sensitive-route surface rather than carving a new exception.

## Decision

**The saved tenant brand is served as an immutable content-hashed CSS asset in the existing [ADR-014 §F](ADR-014-frontend-architecture.md) first-party asset pipeline; the static surface references it via `<link rel="stylesheet" href="/api/v1/tenant/brand.css">` resolving to a Host-keyed tenant-context endpoint that 302-redirects to the immutable hashed URL. Live editor preview is JS-driven CSSOM mutation on the brand-editor sensitive route under per-request nonce CSP. Tenant-overridable scope is bounded to brand primary, brand accent and brand surface tint; status colours, regulatory document chrome and typography are platform-locked.**

### A. Tenant identification on the static surface

Subdomain primary: `{slug}.flight-academy.app` resolves to tenant `{slug}` via Host-header lookup at the edge. Root host (`flight-academy.app`) serves marketing surface with platform-default brand. Self-host runs single-tenant and ignores Host parsing — the only tenant's brand is unconditional. Custom domains are explicitly deferred (see Alternatives).

Path-prefix routing (`/t/{slug}/...`) is rejected at this layer because the static HTML cannot bake its own slug at build time without per-tenant builds — the chicken-and-egg eliminates the simple "every page hard-codes `/api/v1/tenant/brand.css`" link target. Slug-pathed addressing remains the convention for explicit resource access ([ADR-006 §C](ADR-006-api-contract.md)); the brand endpoint is a *session-context* endpoint — "this tenant, derived from where the request came from" — not an explicit lookup, and Host-keying is the natural shape for that.

### B. Saved-brand mechanism

`tenants.settings.brand` JSONB carries `{ primary, accent, surface_tint }` in oklch() form (accent and surface_tint may be null and are derived from primary if so). On save:

1. `flight-academy-brand-emit` produces deterministic CSS bytes from the JSONB — a single `:root { --color-brand: ...; --color-brand-2: ...; --color-brand-soft: ...; --color-brand-ink: ...; }` block, sorted keys, fixed indentation, no comments — byte-identical for byte-identical input.
2. The bytes are SHA-256-hashed; the content-hash alone forms the asset filename: `brand-{hash}.css`. The filename carries no tenant identifier — content-addressed storage means two tenants that happen to choose the same brand JSONB share the same asset file (free deduplication and shared edge cache); tenant context comes from the redirect endpoint (`/api/v1/tenant/brand.css`) gated by `Host`, not from the asset URL.
3. Bytes are written to `flight-academy-store` with `Cache-Control: max-age=31536000, immutable`.
4. The tenant record's `brand_live_asset_id` column is updated to point at the new hash within the same SERIALIZABLE transaction that emits the `tenant.brand.updated` audit event ([ADR-009 §C](ADR-009-event-streams-and-retention.md) tenant chain) with the before/after JSONB diff and the new asset hash.
5. A platform garbage-collection sweeper retires brand assets that are not referenced by any tenant's `brand_live_asset_id` and are older than a 7-day grace period.

The static surface references the brand through a stable endpoint `/api/v1/tenant/brand.css` (Host-keyed; no slug in the URL) that responds `302 Found` with `Location: /assets/brand-{hash}.css` and `Cache-Control: max-age=60, must-revalidate`. The immutable target is the long-cacheable file; the redirect is short-cached so brand updates propagate within ~60 seconds.

**HTTP `103 Early Hints` on the static-content response.** The `apps/api` static handler emits a `103 Early Hints` informational response with `Link: </assets/brand-{hash}.css>; rel=preload; as=style` before the `200 OK` HTML body. The brand asset hash is read from the tenant's `brand_live_asset_id` column — the same lookup that drives the `/api/v1/tenant/brand.css` redirect endpoint. The browser starts fetching the immutable URL concurrently with HTML parse, so by the time it follows the static HTML's `<link>` 302 the resource is in cache or in flight, and the cold-cache penalty approaches the steady-state cost. Browsers without 103 support (Safari <17, Firefox <120 — small minority of current traffic) gracefully ignore the 103 and pay the 2-round-trip cost. As additional tenant-Host-keyed assets join the model later (logo, custom fonts, OG image), the static handler stacks further `Link:` entries in the same 103 response. Infrastructure requirement: the deployed reverse proxy / ingress must forward HTTP 103 unchanged — Envoy, nginx ≥1.13.9, Cloudflare, Fastly and Istio ambient all do. A CI smoke test asserts the static-handler response begins with a 103 carrying at least the brand asset preload hint.

CSP composition: the `<link>` is same-origin external, covered by `style-src 'self'`. No `style-src` directive widens; ADR-015 §B's hash list is unaffected (the static HTML's inline blocks are still build-time-known); ADR-015 §E's style-attribute denial is unchanged.

### C. White-label scope

Tenant-overridable tokens are limited to three colour custom properties; a fourth (`--color-brand-ink`, the text colour on brand surfaces) is derived by the emitter to pass contrast against the chosen brand:

| Token | Editor control | Role |
| --- | --- | --- |
| `--color-brand` | tenant-chosen, required | CTA primary, primary buttons, active states, link hover |
| `--color-brand-2` | tenant-chosen, optional (derived from primary if null) | Secondary highlights, badges, accent |
| `--color-brand-soft` | tenant-chosen, optional (derived from primary if null) | Surface tint for brand-affiliated panels |
| `--color-brand-ink` | **derived by emitter** | Text colour on brand surfaces — emitter picks the contrast-passing choice from a bounded near-white / near-black pair |

Identity assets — logo, favicon, OG image — are uploaded through the existing [ADR-014 §F](ADR-014-frontend-architecture.md) asset pipeline; tenant name appears in chrome via the tenant record's `display_name`.

All other tokens (`--color-bg`, `--color-surface-*`, `--color-ink-*`, `--color-border*`, status `--status-*`, radius `--r-*`, shadow `--shadow-*`, typography) remain platform-defined. Tenants do not choose typography (IBM Plex Sans + Mono per [ADR-014 §F](ADR-014-frontend-architecture.md) is fixed), spacing, density, border-radius or layout structure.

The `tokens.json` source-of-truth (currently marking four `brand-*` tokens as `tenantOverride: true`) is revised by the implementation slice to mark only the three editor-controlled tokens overridable and to flag `brand-ink` as `derivedFrom: brand`.

### D. Dark-mode behaviour

The tenant picks one brand colour. The editor enforces that it passes WCAG AA contrast against both light-mode and dark-mode neutral surfaces (`--color-bg` and `--color-surface` in both modes). A tenant cannot supply per-mode variants, and the system does not derive a dark variant — the colour used is the colour saved.

Rationale: dark-mode variant derivation introduces a hidden transformation tenants cannot inspect; per-mode variants double editor cognitive load. The contrast-both-modes constraint forces tenants to pick colours that work in both, which is a tighter constraint than typical SaaS theming but appropriate when the design system commits to first-class dark mode ([ADR-014 §B](ADR-014-frontend-architecture.md)).

### E. Preview mechanism

Brand-editor preview is **session-local** and **editor-route-internal**. Implementation:

1. The brand-editor route (`/settings/brand`) is a sensitive route under [ADR-015 §C](ADR-015-csp-static-build.md) — per-request nonce in `style-src` and `script-src`.
2. Editor JS manipulates `document.documentElement.style.setProperty('--color-brand', value)` etc. as the user drags colour pickers. CSSOM mutation is not subject to `style-src` enforcement and does not require widening CSP; the nonce-CSP route also tolerates whatever inline styles or scripts the editor's mock pane needs.
3. A `<PreviewPane />` Svelte component renders representative chrome (header, sidebar, primary button states, status chips, sample cards) with the same `var(--color-brand)` references the rest of the app uses. The pane is purely client-local; nothing persists or propagates until Save.
4. Save POSTs the JSONB to the API, which executes §B's compute-write-pointer pipeline.

The preview is the only place dynamic style mutation happens in the codebase. Outside the editor route, the static surface continues to compose under §B hash CSP with the brand `<link>` as the sole tenant-keyed reference.

### F. Locked platform surfaces

The following remain platform-controlled and are not exposed in the editor:

- **Status colour palette** (`--status-ok`, `--status-warn`, `--status-blocked`, `--status-offline`, `--status-info` and their `-soft` counterparts) — safety-critical; common across tenants for the reasons in Context.
- **Regulatory document chrome** — CAA paperwork, MOR submissions, examiner audit packs, ARC reports, Part-145 sign-off documents and any rendering that mimics or carries a regulator-defined layout. These bypass tenant brand entirely; documents identify the tenant via plain-text fields, not visual theming.
- **Typography** — IBM Plex Sans + Mono per [ADR-014 §F](ADR-014-frontend-architecture.md).
- **Spacing, density, border-radius, layout structure** — platform-defined.

### G. Accessibility enforcement

The editor refuses Save when:

- `--color-brand` contrast against `--color-bg` < 4.5 : 1 in either mode
- `--color-brand` contrast against `--color-surface` < 4.5 : 1 in either mode
- `--color-brand-2` contrast against `--color-bg` < 3 : 1 in either mode (lower bar — accent, not primary text)
- `--color-brand-soft` contrast against `--color-ink` < 4.5 : 1 in either mode (the tint is a panel-surface; primary text sits on it)
- `--color-brand-soft` contrast against `--color-ink-2` < 3 : 1 in either mode (secondary text on the tinted panel — lower-prominence WCAG floor)

The platform ink tokens are fixed, so the emitter performs the brand-soft contrast checks automatically against the resolved light- and dark-mode ink values; the tenant cannot pick the ink colours, only the tint they sit on.

Contrast scores are computed live by the editor (WCAG 2.2 relative-luminance formula on the oklch → sRGB result) and displayed alongside each colour picker so tenants see the constraint before hitting Save. The Save endpoint re-validates server-side as defence-in-depth; client validation is UX, not security.

### H. Operations and resilience

- Brand-asset garbage collection runs as a periodic platform job: assets not referenced by any tenant's `brand_live_asset_id` and older than 7 days are deleted.
- Self-healing on missing assets: if a tenant's `brand_live_asset_id` points at a file that no longer exists in `flight-academy-store` (disaster-recovery restore inconsistency), the first read recomputes from JSONB and re-writes; a warning is logged.
- A CI test asserts production CSP retains `style-src 'self'` so the brand `<link>` cannot silently fail to apply due to a CSP regression.

## Consequences

### Positive

- Brand CSS structurally aligns with logo / fonts / OG image — one asset pipeline, one mental model, one set of operational tooling.
- Static surface invariance preserved — every viewer of every page gets the same bytes from the same Host. The `<link>` is the only tenant-keyed reference and it lives outside the hashed inline-content surface.
- CSP unchanged at the directive level — `style-src 'self'` already covers same-origin stylesheets; no new exemption widens the policy, no new hash to maintain.
- Immutable content-hashed assets are CDN-friendly: brand.css can sit on long-cache edges with `max-age=31536000`; only the short-cached redirect endpoint hits origin per page load.
- WCAG AA contrast enforcement at save time prevents the silent-degradation failure mode where a tenant chooses a brand colour that fails accessibility and we only discover it from a support ticket.
- The aviation safety surface (status colours, regulatory documents) remains uniform across tenants.
- Editor preview composes with existing ADR-015 §C sensitive-route nonce policy — no new CSP surface, no new policy to maintain.

### Negative

- Brand-save flow becomes a transactional pipeline across two systems (Postgres + object storage); the pointer swap and asset write must succeed together (or the GC cleans up the orphan). A new operational invariant.
- Garbage-collection sweeper is new background work the platform must run; failure-to-sweep grows storage but does not break functionality.
- First-paint cost on cold cache is bounded by the HTTP `103 Early Hints` mechanism in Decision §B: the brand asset preload begins concurrently with HTML parse, so cold-cache approaches steady-state. The residual cost is the 103-to-200 latency itself (negligible) and the legacy-browser fallback path (Safari <17, Firefox <120 — small minority — pay the 2-round-trip cost on cold cache). Infrastructure must forward HTTP 103 unchanged through every proxy hop, which is well-supported but is a real ops invariant to verify in deploy environments.
- CSS emitter must be deterministic and versioned. Template changes shift hashes for every tenant on next save (or via a one-time bulk recompute) — one more invariant for emitter maintenance.
- Tenant brand colour must pass contrast in both modes — a real constraint that may reject some "brand book" colours. Mitigated by editor UI surfacing the constraint live so tenants understand it before save.

### Neutral

- New per-tenant column `brand_live_asset_id` (nullable text) in `tenants`; new `tenant.brand.updated` audit-event subtype.
- Self-host single-tenant case simplifies the routing layer but exercises the same code path.
- The aviation-specific carve-outs (status colours, regulatory chrome) make this ADR narrower than a typical SaaS white-label decision — recorded so future contributors do not widen scope without re-evaluating the safety argument.

## Alternatives considered

### A.1 — Stream brand CSS directly from the indirection endpoint

`/api/v1/tenant/brand.css` reads `Host`, queries JSONB, emits CSS in-band, returns 200 with the bytes. Simpler — one round trip; no asset pipeline; no GC.

Rejected because (a) it forecloses uniform treatment with logo / fonts / OG image (which need the asset pipeline anyway), (b) CDN economics are weaker — without an immutable hashed URL, the response is short-cached on every edge per tenant, (c) it makes "what CSS served this tenant on this date" a re-derivation question rather than a lookup. The cost of the asset model is bounded and is paid for by structural uniformity.

### Boot-time JS apply via `/api/v1/tenant/brand.json`

Static JS fetches JSON tokens at boot and applies them via `element.style.setProperty` on `:root`. Workable; CSSOM mutation is not CSP-controlled in any major browser today.

Rejected because (a) FOUC — initial paint uses defaults, repaint after JS executes; especially bad for dark-mode-by-default users seeing a light flash, (b) `<noscript>` users see no brand, (c) tenant brand is hidden behind a runtime JS dependency for a problem solvable with a stylesheet link.

### Full-SSR shell on every route

Drop `adapter-static`; SSR every tenant route with per-request nonce so brand can inject inline. Symmetrical to the ADR-015 §C sensitive-route policy applied universally.

Rejected — defeats the static-build purpose of ADR-015 §B (hash CSP, edge caching, single-binary handshake). The whole point of the surface split is that most routes don't need per-request rendering.

### E2 — Whole-app preview via cookie-keyed draft brand

Tenant draft brand persists server-side; a preview cookie causes `/api/v1/tenant/brand.css` to serve the draft to the editor's session while everyone else continues to see live. The editor admin can navigate the full app to see the brand applied before publishing.

Rejected for v1 because (a) it punches a per-session-keyed exception in the static-surface invariance, (b) `Vary: Cookie` defeats the CDN cache for the preview session, (c) requires draft/live state machine, "you're in preview mode" banner mechanism, draft GC. The editor-internal mock pane (Decision §E) is sufficient for a first cut. Reconsider if tenants ask for whole-app preview; the mechanism is well-understood and additive.

### Custom domains — tenant brings their own DNS

`app.bristol-aero.co.uk` resolves to our edge with a tenant-issued TLS cert; users never see `flight-academy.app` in the address bar.

Deferred to a later slice. Pulls in Let's Encrypt ACME automation, domain verification UX, per-tenant cert renewal, monitoring for tenant-DNS drift, landing/hero surfaces per custom domain, email-from complications (SPF/DKIM/DMARC for tenant domain). Worth doing when a tenant is asking; the white-label scope here does not depend on it.

### L3 expansion — curated font choice, login background imagery

Tenants choose from a curated set of 3–5 fonts or upload a login background image.

Deferred. Additive to the asset pipeline; not load-bearing for v1 brand expression. Re-evaluate when accumulated tenant requests justify the scope.

### L4+ expansion — custom font upload, layout / density presets, custom templates

Tenant uploads their own WOFF2 fonts, picks border-radius and density presets, or applies a "template" mirroring their existing web presence.

Deferred. Each is a separate design surface with its own accessibility, performance and visual-cohesion questions, and each pulls in distinct testing matrices.

### L5 — arbitrary CSS or HTML injection

Rejected outright. XSS surface walks past every defence in ADR-015; accessibility floor cannot be enforced; tenant could render a malicious-looking page that erodes trust in the platform.

## References

- Refines [ADR-014 §F](ADR-014-frontend-architecture.md) — replaces the boot-time `<style>` injection mechanism with a `<link>` to an asset-pipeline-served CSS file. The rest of §F (asset pipeline for logos / fonts / OG images, Flutter equivalent reading JSON tokens) is unchanged; this ADR adds derived content (brand CSS) to the same pipeline.
- Clarifies composition with [ADR-015 §B/§C/§E](ADR-015-csp-static-build.md) — `<link>` to same-origin stylesheet is covered by `style-src 'self'` without widening the hash policy or weakening the inline-style-attribute denial. The brand editor is the canonical example of a sensitive-route nonce-CSP page.
- [ADR-006 §C](ADR-006-api-contract.md) — slug-addressed resources; the brand endpoint is Host-keyed rather than slug-pathed because the static HTML cannot bake its own slug at build time, and the brand endpoint is a session-context endpoint not an explicit resource lookup.
- [ADR-009 §C](ADR-009-event-streams-and-retention.md) — tenant chain audit; `tenant.brand.updated` events ride this surface.
- [ADR-014 §B](ADR-014-frontend-architecture.md) — first-class dark mode; the contrast-both-modes constraint in Decision §D implements the practical floor that commitment requires.
- [ADR-016 §C](ADR-016-compliance-baseline.md) — WCAG 2.2 AA commitment; editor-time contrast enforcement implements it for tenant-chosen colours.
- [ADR-017](ADR-017-outbound-http-ssrf.md) — outbound HTTP posture; same reasoning the asset pipeline already uses for logos (no tenant-controlled URLs that the platform would have to fetch).
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 24 (no pretence: dark-mode derivation is not done because the result would be hidden from tenants), 28 (truth: status colours are safety-shaped, not aesthetic), 35–36 (restraint: scope is narrow and deferrals are named).

## Notes

The white-label scope here is intentionally narrower than typical SaaS theming. The reason is domain-specific — aviation safety colours and regulatory document chrome require uniformity across tenants for user safety — and is recorded in Decision §F (Locked platform surfaces). A future contributor reading this who wants to widen the scope should re-evaluate that safety argument first; widening is not a UX preference but a domain-correctness question.

The asset model here treats *derived* content (brand CSS, produced by the emitter from JSONB) the same as *uploaded* content (logos, fonts, OG image, supplied by the tenant as bytes). This is a deliberate uniformity: both go through SHA-hashing, immutable URLs, CDN-cacheable serving and GC of orphans. Adding new derived-asset categories in future (e.g. compiled brand-aware email templates) follows the same pipeline.
