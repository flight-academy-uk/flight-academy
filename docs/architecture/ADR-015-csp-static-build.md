# ADR-015 — CSP and static-build reconciliation

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-02 |
| **Deciders** | @ICreateThunder |
| **Tags** | csp, security, sveltekit, static-build, headers, xss |
| **Supersedes** | (none — resolves the conflict between [ADR-001 §B](ADR-001-platform.md) and [ADR-004 §F](ADR-004-defence-in-depth.md)) |

## Context

[ADR-001 §B](ADR-001-platform.md) commits to SvelteKit `adapter-static`
for the tenant web app, served via `apps/api` embedded-static mode
([ADR-002 §H](ADR-002-release-deployment.md)).
[ADR-004 §F](ADR-004-defence-in-depth.md) commits to a strict Content
Security Policy with **nonce-based inline allowance** for inline
scripts and styles. These conflict directly: pre-built static HTML
cannot carry a fresh per-request nonce — the HTML is baked at build
time.

Three families of resolution exist (hash-based CSP, dynamic adapter
for sensitive routes, deny inline entirely). Each trades attack-surface
reduction against build complexity and developer ergonomics.

Adjacent forces: [ADR-014](ADR-014-frontend-architecture.md) defines
two SvelteKit codebases with different threat models. Tenant codebase
is internet-facing and largely static; staff plane is internal-only
and may benefit from stricter policy.
[ADR-011](ADR-011-user-consent-grant.md) defines OAuth consent screens
needing protection from clickjacking and code injection.
[ADR-001 §F](ADR-001-platform.md) passkey ceremonies operate at
sensitive routes that handle credential material.

## Decision

**Hash-based CSP for the static tenant surface; dynamic per-request
nonce for sensitive tenant routes (login, magic-link verify, passkey
ceremony, OAuth consent); per-request nonce for the entire staff plane.
Inline-style HTML attributes (`style="..."`) are denied across all
surfaces — Tailwind compile-time classes only. Hash extraction runs in
the build pipeline; resulting hashes ship in the CSP header.**

### A. Three surfaces, three policies

| Surface | Serving mode | Inline policy |
| --- | --- | --- |
| Tenant static routes (most of `apps/web`) | Pre-built via `adapter-static`, served by `apps/api` | **Hash-based** — SHA-512 hashes of all inline `<script>` and `<style>` enumerated in `Content-Security-Policy` |
| Tenant sensitive routes (login, magic-link, passkey, OAuth consent) | SvelteKit endpoint with SSR (`+page.server.ts`) | **Per-request nonce** — `'nonce-{random}'` in CSP, matching `nonce={nonce}` on inline tags |
| Staff plane (`apps/admin-web` in its entirety) | Dynamic-served by `apps/admin` | **Per-request nonce** — staff plane has no static-build mode; every page SSR |

All three surfaces share the rest of the CSP: `default-src 'self'`;
`object-src 'none'`; `frame-ancestors 'none'`; `base-uri 'self'`;
`form-action 'self'`; `connect-src 'self'` plus allow-listed API
origins; `img-src 'self' data:` plus allow-listed CDNs.

### B. Hash-based CSP for the tenant static surface

After the SvelteKit static build:

1. A post-build script (`apps/web/scripts/extract-csp-hashes.ts`)
   walks `apps/web/build/`, extracts every inline `<script>` and
   `<style>` block, computes SHA-512 hashes, writes the list to
   `apps/web/build/csp-hashes.json`.
2. The API binary reads `csp-hashes.json` at startup (loaded via
   `rust-embed` alongside the static assets).
3. The static-route response handler emits `Content-Security-Policy`
   with `script-src 'self' 'sha512-{hash}'` for each entry.

Hash extraction is deterministic — the same build produces the same
hashes. CI verifies that no inline script/style exists without a
corresponding hash; build fails if an unhashed block appears.

**Rotation is automatic** — every build refreshes the list. Size is
small (typically <30 entries).

### C. Per-request nonce for sensitive routes

Sensitive routes — login, magic-link verify, passkey ceremony, OAuth
consent ([ADR-011](ADR-011-user-consent-grant.md)) — are handled via
SvelteKit `+page.server.ts` / `+server.ts` endpoints with `csr=false`
to ensure HTML is generated per request:

1. The handler generates a 256-bit random nonce (Web Crypto API in
   SvelteKit, `OsRng` in `apps/api`).
2. The HTML template injects `<script nonce={nonce}>` and
   `<style nonce={nonce}>` on inline blocks.
3. The `Content-Security-Policy` header includes `'nonce-{nonce}'` in
   `script-src` and `style-src`.
4. The nonce is single-use — different per request, never reused,
   never cached.

**No mixed mode.** A sensitive route is either fully nonce-driven (and
may have any inline content) or it falls under the static-route hash
policy. The route decides at definition time.

### D. Staff plane CSP

The staff plane has no static-build mode. Every `apps/admin-web` route
is dynamic SSR via `adapter-node` (or equivalent), with per-request
nonce on every page. Consistent with the staff plane's threat model
([ADR-010 §I](ADR-010-platform-operator-access.md)) — internal,
authenticated, low-volume, latency-tolerant.

Additionally, the staff plane CSP uses **`'strict-dynamic'`** —
allowing the nonced root script to load further scripts dynamically
without listing each, reducing maintenance.

### E. Inline-style attribute denial

`style="..."` HTML attributes are denied on all surfaces by omitting
`'unsafe-inline'` from `style-src` and not exempting attribute styles.
This means:

- All styling goes through Tailwind classes (compiled at build time)
  or CSS custom properties ([ADR-014 §F](ADR-014-frontend-architecture.md)
  white-label tokens).
- Dynamic colour/size/position changes happen via class toggling or
  CSS custom property updates, not inline `style=`.
- Svelte component code using `style:` directives or inline `style=`
  strings fails the build (lint rule).

A real ergonomic constraint. It exists to remove a common XSS vector —
attacker-controlled values landing in `style=` attributes.

### F. Build pipeline integration

```text
1. apps/web-ui builds tokens → tokens.css, tokens.dart
2. apps/web SvelteKit build → apps/web/build/
3. apps/web/scripts/extract-csp-hashes.ts → apps/web/build/csp-hashes.json (current build)
4. Fetch previous deploy's csp-hashes.json via GitOps reference and
   merge → csp-hashes.json contains current ∪ previous (see H)
5. CI lint: verify every inline script/style in build/ has a hash entry
6. apps/api cargo build --features embedded-static → bakes build/ in
7. apps/api at startup loads csp-hashes.json from rust-embed
8. apps/api static handler emits CSP header per request with the union
```

Equivalent for the staff plane omits 3, 4, 6, 7 — `apps/admin-web`
runs its own server with per-request nonces.

**Previous-deploy fetch mechanism (step 4).** The previous-deploy
hash list is sourced from the **GitOps reference** to current
production: the CI workflow reads the infra repository's
`kustomization.yaml` (or equivalent Flux/ArgoCD-tracked manifest),
extracts the current production image SHA, pulls that image, and
extracts its embedded `csp-hashes.json`. This reflects **what is
actually deployed**, not what was last built — aligned with the Flux
and ArgoCD posture in [ADR-002](ADR-002-release-deployment.md).
Graceful fallback: if the GitOps state is unreadable (first deploy,
disaster recovery, transient network failure), CI logs a warning and
proceeds with current-only hashes; the build completes but cached
HTML from prior versions will see CSP mismatches until they refresh.

**`csp-hashes.json` schema is algorithm-aware** to permit future
hash-algorithm transitions without a JSON-format break:

```json
{
  "sha512": ["abc123...", "def456..."]
}
```

A future migration could add `"sha256": [...]` during a transition
window or replace the algorithm entirely; the serving code reads each
algorithm key, emits the corresponding `'sha{N}-{hash}'` CSP source
expressions, and ignores unknown keys. Self-describing schemas are
cheap to add now and expensive to retrofit.

### G. Failure modes

- **Hash drift.** A SvelteKit dependency update introduces a new
  inline block; build fails at the lint step until hashes are
  refreshed. By design — we want to know.
- **Nonce leak.** A sensitive route accidentally caches rendered HTML
  with its nonce; replay attacker uses the cached nonce. Mitigation:
  sensitive routes set `Cache-Control: no-store`; the CSP nonce is
  bound to the response, not the route.
- **CSP bypass via misconfigured `'unsafe-eval'` or `'unsafe-inline'`.**
  Build-time CI test asserts production CSP has neither directive on
  any surface.
- **Static route attempts to inject runtime content.** A future feature
  might want tenant-specific JavaScript at runtime on a static page;
  this conflicts with hash-based CSP. Mitigation: that page must move
  to sensitive-route serving — change at design time, not by quietly
  weakening CSP.
- **Staff plane CSP drift.** `'strict-dynamic'` is powerful but easy to
  weaken accidentally. Build-time CI test asserts staff plane CSP
  retains the nonce + strict-dynamic shape.

### H. Rollover discipline — surviving canary and multi-version overlap

Each binary version emits self-consistent header+body — any single
response from a single pod is intrinsically correct. The risk is
**cross-response inconsistency** when two binary versions serve
traffic concurrently:

- **Flagger canary at the Istio level**
  ([ADR-002](ADR-002-release-deployment.md)) splits requests between
  stable (N) and canary (N+1) pods by weight. No sticky-session by
  default — a user can receive HTML from N for one request and a CSP
  header from N+1 on a later cached HTML revalidation.
- **Browser / CDN cache** retains HTML from N across the deploy
  boundary; user receives a fresh CSP from N+1 against cached old
  inline scripts.

Failure signature: browser console reports `Refused to execute inline
script because it violates the following Content Security Policy
directive` — looks like an XSS attempt, is actually stale cache.

Three commitments close this:

- **Union-of-hashes in the CSP header.** Each build's `csp-hashes.json`
  includes the current build's hashes **plus the previous shipped
  build's hashes**. The previous deploy's hash set is sourced via
  the GitOps reference described in §F (CI reads the infra repo's
  Flux/ArgoCD-tracked manifest, pulls the current production image,
  extracts its embedded `csp-hashes.json`, and merges). After two
  successful deploy cycles the union prunes naturally (N's "previous"
  is N-1; N+1's "previous" is N). For rapid-deploy scenarios
  (≥3 deploys within HTML cache TTL), include N-2 as well —
  implementation detail recorded in
  `docs/operations/deployment.md` (TBD).
- **Cache discipline.** HTML responses set short `Cache-Control` with
  `ETag` revalidation (e.g. `max-age=60, must-revalidate`).
  Hashed-filename static assets retain long TTL (`max-age=31536000,
  immutable`). The hashed names handle invalidation; HTML uses
  revalidation.
- **CDN distribution pinning.** If CloudFront or another CDN ever
  fronts `apps/api` in a blue-green configuration, each distribution
  must pin to its corresponding `apps/api` version; cross-distribution
  request mixing within a single page session is forbidden.

A service worker, if ever introduced
([ADR-014](ADR-014-frontend-architecture.md) §A leaves this out of
v1), must invalidate cached HTML on new-version detection — otherwise
the SW serves cached HTML against fresh network CSP and produces the
same failure mode.

### I. CVE emergency-break case

The deliberate inverse of §H's rollover discipline. When a critical
(CVSS 9+) frontend vulnerability is discovered, the goal is to force
old cached clients into hard failure, not to preserve their
compatibility. **The load-bearing mechanism is emergency session-key
rotation** — every other layer raises attacker cost or stops
re-infection but does not by itself stop a compromised JS still
holding a valid session.

**Trigger mechanism.** A dedicated CI workflow
(`.github/workflows/cve-emergency.yaml`) hosts the emergency-build
path, separate from the normal release pipeline. The workflow:

- Is invoked via `workflow_dispatch` with a required
  `cve_advisory_url` input and an `incident_id` input.
- Is configured with **required-reviewer protection** in repository
  settings — cannot run without sign-off from a maintainer outside
  the invoker.
- Sets `EMERGENCY_BREAK=true` in the build env, which causes the
  previous-deploy hash fetch (§F step 4) to be **skipped** rather
  than executed; the `csp-hashes.json` ships current-only.
- Records the trigger event to the platform audit chain
  ([ADR-010 §E](ADR-010-platform-operator-access.md)) including the
  CVE advisory URL, incident ID, and the user who approved.

The separate-workflow design enforces deliberate ceremony — the
emergency path cannot run accidentally from a normal-release CI
trigger.

**Procedure (high-level):**

- **Emergency-rotate the tenant API session signing key**
  ([ADR-013 §F](ADR-013-auth-keys.md)) — every existing JWT becomes
  unverifiable; refresh attempts fail; access tokens expire within 10
  minutes; the compromised JS loses session access regardless of what
  it claims. **This is the load-bearing step.**
- Run the emergency CI workflow to build and deploy the patched
  version with `EMERGENCY_BREAK=true` — the union becomes the current
  build only, so cached vulnerable HTML's inline scripts get blocked
  by the new CSP. Closes re-infection of fresh navigations against
  stale HTML.
- Purge the vulnerable bundle's hashed-filename file from the static
  asset store — old clients 404 their bundles, hydration fails, page
  breaks visibly. Closes re-infection of any path that retries asset
  loads.
- Raise `minimum_supported_version`
  ([ADR-006 §J](ADR-006-api-contract.md)) — refuses API requests from
  benign stale clients and forces a clean refresh; also a cost-raiser
  against actively-compromised JS, which can spoof the header but is
  bounded by the server-side allow-list and JWT-attested version
  ([ADR-006 §J](ADR-006-api-contract.md)).
- Notify affected tenants via the real-time transparency channel
  ([ADR-010 §J](ADR-010-platform-operator-access.md)).

Each non-rotation layer raises attacker cost (more reconnaissance,
narrower window, harder spoofing) without claiming to terminate an
active compromise. Rotation does the terminating. The runbook order
and comms templates live in `docs/operations/incident-response.md`
(TBD).

## Consequences

**Positive.** Strong CSP across all surfaces with no `'unsafe-inline'`
anywhere. Static surface stays static (build-cache friendly, no
per-request render cost). Sensitive routes get the strongest available
protection (per-request nonce + SSR). Staff plane benefits from
`'strict-dynamic'`. XSS attack surface meaningfully reduced.
Compatible with [ADR-002 §H](ADR-002-release-deployment.md)
embedded-static (the hash list is embedded too).

**Negative.** Build pipeline gains a hash-extraction step, a CI
lint, and a previous-deploy fetch (§F, §H). Inline-style attribute
denial is a real developer constraint — porting JSX with inline
styles requires class extraction. Sensitive routes must be SSR —
can't be statically pre-rendered (slower TTFB by tens of ms, fine
for routes used once per session). Hash drift on dependency updates
triggers build failures (intentional, expected occasionally). CSP
header is roughly 2x larger during deploy transitions while the
union spans two builds.

**Neutral.** Three policies sound like three things to maintain; in
practice they share most of the CSP and differ only in inline
allowance. Per-request nonce generation is microseconds.

## Alternatives considered

- **Hash-based CSP everywhere (no nonce, no dynamic adapter).**
  Simpler — one build, one policy. Rejected: sensitive routes
  sometimes inject runtime values (e.g. the user's passkey
  challenge), which a static build can't do. Forcing all dynamic data
  through fetch + DOM is feasible but loses some CSP defence (the
  manipulation script must be allowed).
- **Per-request nonce everywhere.** Strongest uniform policy.
  Rejected: requires `adapter-node` for all routes, breaks `apps/api`
  embedded-static ([ADR-002 §H](ADR-002-release-deployment.md)),
  forces self-host to run SSR. Static surface is a real cost win we
  shouldn't lose.
- **Deny all inline (no hashes, no nonces).** Strongest possible.
  Rejected: SvelteKit's hydration emits a small inline script
  expensive to externalise; some Svelte component patterns rely on
  inline `<style>` for scoped CSS. Pragmatic compromise: allow hashed
  inline.
- **CSP report-only mode initially.** Run report-only before
  enforcing. Worth doing as a launch tactic, but the architecture is
  enforced-mode from the start; report-only is a deployment concern.
- **`'strict-dynamic'` on the tenant surface too.** Lets scripts load
  further scripts. Rejected on the static surface: hash-listed inline
  scripts shouldn't be loading more at runtime — that's the loophole;
  `'strict-dynamic'` weakens hash policy meaningfully. Kept on staff
  plane where the threat model permits.

## References

- [ADR-001 §B/§F](ADR-001-platform.md) — refined here:
  adapter-static for static surface, dynamic for sensitive routes;
  passkey ceremony is a sensitive-route example.
- [ADR-002 §H](ADR-002-release-deployment.md) — embedded-static
  mode; build pipeline §F integrates here.
- [ADR-004 §F](ADR-004-defence-in-depth.md) — refined here: nonce-CSP
  becomes nonce-on-sensitive + hash-on-static.
- [ADR-010 §I](ADR-010-platform-operator-access.md) — staff plane
  runtime topology; §D specifies its CSP.
- [ADR-011](ADR-011-user-consent-grant.md) — OAuth consent screens
  are sensitive routes.
- [ADR-014](ADR-014-frontend-architecture.md) — two SvelteKit
  codebases; this ADR specifies their CSP behaviour.
- [ADR-016 §E](ADR-016-compliance-baseline.md) — OWASP ASVS L2 / NCSC
  Cloud Security Principles alignment.
- MDN CSP Level 3 — <https://developer.mozilla.org/en-US/docs/Web/HTTP/CSP>.
- W3C CSP 3 — <https://www.w3.org/TR/CSP3/>.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 28
  (truth — the role-assignment table is honest about which layer
  terminates an attack vs raises cost; "blocked by CSP" can mean
  stale cache, not always attack), 34 (be not proud — SHA-512 over
  SHA-256 is over-target with honest framing), 35–36 (restraint —
  three policies share most of the CSP; inline-style denied; v1
  scope bounded), 38 (be not lazy — build-time hash extraction with
  failing CI lint; required-reviewer protection on the
  emergency-break workflow), 48 (watchfulness — defence-in-depth;
  rollover and emergency-break both designed; failure modes named).

## Notes

The most reversible part of this ADR is the staff plane's
`'strict-dynamic'`. If a real failure mode emerges (a staff
dependency loads scripts the policy didn't anticipate),
strict-dynamic can be replaced with explicit nonces per script tag at
modest cost.

The most load-bearing part is the inline-style attribute denial.
Walking it back later would mean re-auditing every PR for the
preceding year to find injection vectors; better to enforce from day
one.

SHA-512 chosen over SHA-256 as a consistent over-target with the
project's design-aligned posture
([ADR-016 §C](ADR-016-compliance-baseline.md)). SHA-256 would have
been sufficient on threat-model grounds — CSP hashes are rotated
every build, the second-preimage attack is the realistic threat
(infeasible at SHA-256 even post-quantum via Grover), and inline
script counts are small (~3-10 per page, ~30 site-wide). The cost of
the stronger choice is bounded: ~1KB extra header bytes (compresses
in HPACK/QPACK), microseconds of browser-side compute per page,
slight departure from CSP example conventions. Distinct from
[ADR-013](ADR-013-auth-keys.md) §B's bounded-TTL exception (sessions
stay Ed25519 because rotation is the security boundary) — there the
cost of stronger primitives would be header bytes per *every API
request* across millions; here it's per *static asset response* with
HPACK/QPACK compression closing the gap. Different cost curves,
different choices.
