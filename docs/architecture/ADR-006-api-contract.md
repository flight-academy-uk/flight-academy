# ADR-006 — API contract conventions

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | api, openapi, rest, versioning, webhooks, idempotency, contract |
| **Supersedes** | (none) |

## Context

[ADR-001 §A](ADR-001-platform.md) decided the API *style* (REST + OpenAPI 3.1
via utoipa, path versioning, tenant in path, generated SDKs). It did not pin
day-to-day conventions. Flight Academy ships a **public developer API** with
third-party consumers (domain-model §2.13); a published shape is a promise.
This ADR fixes one convention each for the things consumers depend on.

Forces: the spec must never drift from the server; conventions must hold across
SvelteKit (cookie + JWT), Flutter (bearer + offline), and third-party scoped
keys without special cases; restraint ([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md)
instruments 35–36 — one of each); truth (instrument 28 — the spec is a public
statement, emitted from code, diffed in CI). The portal mockup's
tenant-implicit `/v1/bookings` overshoots ADR-001 §A; this ADR resolves it (C).

## Decision

**The OpenAPI 3.1 spec is emitted from utoipa-annotated Axum handlers and is
the single source of truth. The server is never generated from a spec; clients
are always generated from the emitted spec. One convention each for versioning,
addressing, representation, mutation safety, errors, auth, and webhooks.**

### A. Pipeline — design-first, code-first emission, generated clients

1. **Design** the contract from [domain-model.md](../design/domain-model.md).
2. **Hand-write** thin Axum stubs + types, annotated with `utoipa`, registered
   via `utoipa-axum`'s `OpenApiRouter` / `routes!` so the route and its spec
   fragment register together.
3. **Emit** the OpenAPI document from `apps/api` and commit it at
   `docs/api/openapi.yaml` so every PR diffs the contract.
4. **Generate clients** from the committed spec: TS web via `openapi-typescript`
   + `openapi-fetch`; Dart mobile via `openapi-generator dart-dio`. Python SDK
   when there is demand.
5. **Implement** handler bodies against domain crates.

Server is the source of truth; spec is its shadow; clients are the spec's
shadow. Drift is structurally impossible — no link is authored independently.
CI fails a PR whose committed spec doesn't match what the handlers emit.

**Spec version.** Committed spec is OpenAPI **3.1**. 3.2.0 (Sep 2025) is a
backward-compatible superset, but the toolchain (utoipa, `openapi-typescript`,
`openapi-generator` via `swagger-parser`) is 3.1-bound as of mid-2026 — 3.2
read support in `openapi-generator` is tracked as
[OAI/openapi-generator#22728](https://github.com/OpenAPITools/openapi-generator/issues/22728),
blocked on `swagger-parser`. Revisit when the toolchain catches up **and** a
3.2 feature is needed (most likely streaming). The upgrade is compatible; no
major-version bump.

### B. Versioning — path-based major, additive doesn't bump

URL carries the major: `/api/v1/…`. Rules:

- **Backward-compatible additions don't bump**: new endpoint, optional request
  field, response field, new output enum value (consumers must tolerate
  unknowns — stated in the docs).
- **Breaking changes require `/api/v2`**: removing/renaming a field, type
  change, making optional required, removing an endpoint, narrowing an output
  enum.
- **Deprecation**: `deprecated: true` in the spec + `Sunset` HTTP header
  (RFC 8594) + changelog. **Minimum sunset window: 12 months** from
  announcement; security-driven removals may be shorter but must carry a
  security advisory. Removal lands in the next major after the window.
- **Dated label** (`v2026-05`) on the portal is a release snapshot for changelog
  and SDK-release naming, not a second versioning axis.

### C. Addressing — three planes, tenant in path

| Plane | Prefix | Tenant resolution |
| --- | --- | --- |
| Tenant | `/api/v1/tenants/{tenant}/…` | Path segment, validated against the caller's tenant (JWT claim first-party; key scope third-party). |
| Personal | `/api/v1/me/…` | Authenticated user; no tenant segment. |
| Public | `/api/v1/public/…` | None; anonymous or capability-token (e.g. CV verify code). |

The portal's tenant-implicit `/v1/bookings` is **rejected**. Even with a
tenant-scoped key, the `{tenant}` path segment must match the key's tenant or
the request is `403` — deliberate defence in depth. Cross-tenant queries
remain out of scope (ADR-001 §A); a multi-org user makes per-tenant calls;
the personal cross-org dashboard is a client-side fan-out over `/me/…` plus
per-tenant reads. `{tenant}` is the **slug**, not the UUID.

This **refines [ADR-001 §A](ADR-001-platform.md)** (adds the `/me/…` and
`/public/…` planes) and [§F](ADR-001-platform.md) (a JWT serving `/me/…`
carries no `tenant` claim or `tenant: null`); the per-request tenant claim
continues to hold for `/tenants/…` requests as §F specifies. Same
reconciliation pattern as ADR-005's `fa-*` table.

### D. Representation — JSON, IDs, time, pagination

- **Naming**: `snake_case` in JSON; matches the Rust structs via utoipa.
- **Identifiers**: opaque prefixed strings at the API boundary — `bk_`
  (booking), `ac_` (aircraft), `usr_`, `ten_`, … — mapping to internal UUIDs.
  Prefixes catch wrong-type IDs as `400` and decouple wire from storage.
- **Timestamps**: every `*_at` is a UTC instant in RFC 3339 (Z-suffix; never
  ambiguous). Where a domain concept needs *local* time (airfield-local
  booking slots, daylight scheduling, METAR/NOTAM), the resource carries an
  additional `time_zone` (IANA) and a `*_local` companion; UTC is always
  authoritative.
- **Pagination**: **cursor-based**, never offset. Responses:
  `{ data: [...], meta: { next_cursor, total? } }`; requests take `limit`
  (1–100, default 20) and `cursor`. Cursors encode a unique total order
  (`(sort_key, id)` tuple) so equal sort values cannot cause skips or
  duplicates.
- **Money**: integer minor units (pence) + ISO 4217 `currency`. Never floats.
- **Filtering & incremental sync**: curated indexed filter set per resource +
  `updated_since` change feed — see [ADR-007](ADR-007-sync-filtering-deletion.md).

### E. Mutation safety

- **HTTP semantics**: `GET` safe; `PUT`/`DELETE` idempotent; `POST` creates or
  triggers; `PATCH` partially updates. No writes through `GET`.
- **Idempotency keys**: every unsafe write that creates a resource or triggers
  an external effect accepts an `Idempotency-Key` request header
  (client-generated UUID). Server persists key + result; retries return the
  original. Counterpart to the outbox idempotency
  ([ADR-003 §D](ADR-003-db-migrations.md)). **Required** for the mobile client
  whose offline queue replays writes on reconnect.
- **Offline-created entities**: mobile may generate a client-side UUID and
  `PUT` to a deterministic path so a replayed create is naturally idempotent;
  the `Idempotency-Key` header covers cases where it isn't.
- **Optimistic concurrency (deferred)**: multi-writer authoritative records
  will use `ETag` + `If-Match` (→ `412` on stale). Not needed for v1's
  single-writer resources; becomes required if external write-back lands
  ([ADR-008](ADR-008-data-sharing-posture.md) Notes).
- **Long-running operations (deferred)**: bulk exports / large backfills
  follow AIP-151 (`202 Accepted` + `jobs/{id}` poll or webhook). Not built
  now; named so a contributor reaches for the standard.

### F. Errors — RFC 9457 problem+json, one envelope

Every error is `application/problem+json` per RFC 9457:

```json
{
  "type":     "https://flight-academy.app/problems/booking-conflict",
  "title":    "Booking conflict",
  "status":   409,
  "detail":   "The aircraft is already booked for an overlapping slot.",
  "instance": "/api/v1/tenants/oxford/bookings",
  "request_id": "..."
}
```

The canonical `Error` enum in `flight-academy-core`
([ADR-005 §C](ADR-005-workspace-layout.md)) implements `IntoResponse` to render
this envelope. `type` URIs are stable and documented. `request_id` echoes
`x-request-id` from [ADR-004 §B](ADR-004-defence-in-depth.md). Validation
failures use the problem-details extension for field-level errors.

### G. Authentication & authorisation surface

| Caller | Credential | Notes |
| --- | --- | --- |
| First-party web | HttpOnly cookie + double-submit CSRF | ADR-001 §F |
| First-party mobile | `Authorization: Bearer <jwt>` + opaque refresh | ADR-001 §F |
| Third-party (tenant) | `Authorization: Bearer fa_sk_{live,test,sandbox}_…` | tenant-scoped API key |
| Third-party (user) | `Authorization: Bearer <jwt>` from OAuth 2.1 grant | user-consent token; see [ADR-011](ADR-011-user-consent-grant.md) |

- **Scopes** name `resource:action` (`bookings:read`, `webhooks:manage`). Keys
  are issued a subset; a call outside scope is `403`. Scopes gate the key;
  ABAC ([ADR-001 §C](ADR-001-platform.md)) still evaluates per request.
  Scopes are **sensitivity-graded** ([ADR-008](ADR-008-data-sharing-posture.md)):
  default narrow operational; reading personal data needs an elevated grade;
  bulk `…:sync` is its own off-by-default class. No scope grants
  special-category data or user-owned data without the owning user's consent
  ([ADR-011](ADR-011-user-consent-grant.md)).
- **Key tenant-scope** must match the `{tenant}` path segment (C).
- **Rate-limit headers**: `X-RateLimit-{Limit,Remaining,Reset}`; `429` carries
  `Retry-After`. Surfaces `tower_governor` ([ADR-004 §B](ADR-004-defence-in-depth.md)).
- **CORS**: pre-flight cached 24h (`Access-Control-Max-Age: 86400`).
  - Same-origin first-party — allowed with credentials (cookies).
  - Cross-origin with an **API key** — caller origin must be on the key's
    `allowed_origins` list (set at issue time); checked per pre-flight, no
    wildcard.
  - Cross-origin with a **user-grant token** ([ADR-011](ADR-011-user-consent-grant.md))
    — the OAuth client's registered `redirect_uris` host(s) form the
    allow-list.
  - `/me/…` is **same-origin only** — third parties reach it via ADR-011
    grants, not direct CORS.
  - Public surfaces (`/api/v1/public/*`, spec download, status) return `*`
    with no credentials.
  - Whitelisted request headers: `Authorization`, `If-Match`,
    `Idempotency-Key`. Exposed response headers: `X-RateLimit-*`, `Sunset`.

### H. Webhooks — naming, signing, delivery

- **Event names**: `resource.event`, past-tense (`booking.created`,
  `aircraft.aog`, `safety.report.submitted`). Prefix wildcards
  (`booking.*`).
- **Source**: the outbox ([ADR-003 §D](ADR-003-db-migrations.md)). A webhook
  is one outbox handler; notifications are another. One spine, two
  deliveries.
- **Signing**: HMAC over the raw body + timestamp, headers per
  [ADR-001 §E](ADR-001-platform.md) / [ADR-004 §F](ADR-004-defence-in-depth.md).
  Receivers verify before trusting. Fixed egress IPs
  ([ADR-002 §G](ADR-002-release-deployment.md)) let tenant firewalls allow-list.
- **Delivery**: at-least-once with retries + exponential backoff; stable
  `event_id` for consumer dedupe. Delivery health visible in the developer
  console.
- **Payloads** carry the same resource representation as the REST API.

### I. Data sensitivity

v1 uses server-side envelope encryption (ADR-001 §D), so **no resource is an
opaque client-encrypted blob** — every spec field is server-readable.
Future client-side E2E (community chat is the candidate) would mark the
payload as opaque ciphertext in the spec.

### J. Client version and emergency invalidation

First-party clients send an `X-Client-Version` request header on every
API call, populated at build time
(e.g. `X-Client-Version: 2026-06-03-abc1234`). The header carries the
build-stamp emitted by the SvelteKit / Flutter build pipeline. Third-
party clients (OAuth grants, tenant API keys) are not required to send
it; their version policy is governed by the OAuth client lifecycle
([ADR-011 §D](ADR-011-user-consent-grant.md)).

The API maintains a `minimum_supported_version` policy. Below the
minimum, the response is:

```text
HTTP/1.1 426 Upgrade Required
Content-Type: application/problem+json
```

```json
{
  "type": "https://flight-academy.app/problems/client-out-of-date",
  "title": "Client version no longer supported",
  "status": 426,
  "detail": "Please reload to upgrade. Your current version cannot safely access the API.",
  "instance": "/api/v1/tenants/oxford/bookings",
  "minimum_supported_version": "2026-06-03",
  "request_id": "..."
}
```

The wrapped client ([ADR-014 §E](ADR-014-frontend-architecture.md))
recognises `426` and surfaces a forced-refresh UI; ongoing API calls
fail until reload.

**Trust boundary.** `X-Client-Version` is **client-controlled and
spoofable**. An attacker who has actively compromised the JS can claim
any value. The header is therefore an effective defence against
**benign stale clients** (cached HTML, background tabs faithfully
reporting their build stamp) — the common case — but **not against
active compromise**. The load-bearing defence against active
compromise is emergency session-key rotation
([ADR-013 §F](ADR-013-auth-keys.md)), which invalidates all existing
sessions regardless of what any client claims.

Two cost-raising defences accompany the version header. Neither is
load-bearing; together they make spoofing meaningfully harder:

- **Server-side allow-list of known build stamps.** The API maintains
  the set of actually-deployed build stamps (most recent N — e.g. the
  last 30 days of releases). `X-Client-Version` is checked against the
  set; values outside it return `426`. An attacker can no longer claim
  a far-future date for safety; they must discover and use a currently-
  deployed stamp. Discovery requires probing (logged and
  detectable). Cost: a small in-memory set, populated by CI on every
  deploy. Effect: closes the trivial "claim a year-2099 stamp"
  bypass.
- **Server-attested version in the session JWT.** The login page is
  server-rendered ([ADR-015 §C](ADR-015-csp-static-build.md)) and
  embeds an HMAC-signed `build_attestation` token tied to the build
  the server just served. The client submits the token with
  credentials; the server validates it and stamps the resulting JWT
  with the attested `client_build_id`. Future API calls trust the
  JWT's stamped version (server-signed) rather than the
  `X-Client-Version` header. The attestation cannot be forged by
  compromised JS — it requires the server's signing key
  ([ADR-013 §B](ADR-013-auth-keys.md)). Effect: closes the
  "compromised JS triggers re-login with a fake claim" bypass.

These layers compose with session-key rotation and asset purge to
form the kill-switch. **The honest role assignment:**

| Layer | Defends against | Spoofable? |
| --- | --- | --- |
| `X-Client-Version` header + `minimum_supported_version` gate | Benign stale clients | Yes — client-controlled |
| Server-side build-stamp allow-list | "Claim arbitrary future date" trivial spoof | No — server-controlled set |
| Server-attested JWT version (login attestation) | "Compromised JS re-logs with fake claim" | No — server-HMAC'd |
| Emergency session-key rotation ([ADR-013 §F](ADR-013-auth-keys.md)) | **Active compromise — load-bearing** | No — server-controlled |
| Asset purge + CSP-hash exclusion ([ADR-015 §I](ADR-015-csp-static-build.md)) | Future infections; stale-cached new sessions | No — server-controlled |

The operational procedure (order of operations, comms templates,
tenant transparency notification via
[ADR-010 §J](ADR-010-platform-operator-access.md) real-time channel)
lives in `docs/operations/incident-response.md` (TBD).

## Consequences

**Positive.** Drift structurally impossible. Third parties get a real
contract: versioning, deprecation, cursor pagination, idempotency, one error
envelope, signed webhooks. One shape to learn. Offline-safe by construction.
Errors correlate with logs via `request_id` (no PII).

**Negative.** Prefixed-ID mapping is extra code. Idempotency-key storage is
state to keep. The emit-and-diff CI step adds time. Deprecation discipline is
ongoing editorial work.

**Neutral.** `openapi-typescript` over a heavier client generator suits a
static SvelteKit build. RFC 9457 ties us to a standard media type (upside:
tooling already understands it). Cursor pagination gives up random page
access — fine for our data.

## Alternatives considered

- **Spec-first + server codegen** — Rust server generation via
  `openapi-generator` is non-idiomatic and drift-prone; the spec becomes a
  second source of truth that diverges silently.
- **Header / media-type versioning** — hides version from logs/caches/casual
  reading; path versioning (ADR-001 §A) is cacheable and visible.
- **Offset pagination** — unstable under concurrent inserts; deep offsets are
  costly in Postgres.
- **Raw UUIDs as API IDs** — simpler at the boundary but prefixes catch
  wrong-type IDs early and decouple wire from storage for a public-SDK API.
- **Bespoke error shape** — every consumer would special-case it; RFC 9457
  is the standard with extension points.
- **GraphQL** — already rejected in ADR-001 §A (attack surface, caching,
  rate-limiting). Not reopened here.

## References

- [ADR-001 §A/§C/§E/§F](ADR-001-platform.md) — keel: API style, ABAC, webhook idempotency, sessions/credentials.
- [ADR-003 §D](ADR-003-db-migrations.md) — outbox + idempotency-key state machine that E rides.
- [ADR-004 §B/§F](ADR-004-defence-in-depth.md) — `tower_governor` headers (G); `x-request-id` (F); webhook signing (H).
- [ADR-005 §C](ADR-005-workspace-layout.md) — `Error` pattern in `flight-academy-core` rendering F.
- [ADR-007](ADR-007-sync-filtering-deletion.md) — `updated_since`, curated filters, delete/erase (D filtering pointer).
- [ADR-008](ADR-008-data-sharing-posture.md) — posture grading G scopes; data classes behind I; LRO/concurrency deferrals trace back here.
- [ADR-009](ADR-009-event-streams-and-retention.md) — sizes outbox H feeds; refines audit scope.
- [ADR-010](ADR-010-platform-operator-access.md) — staff plane is out of this product API.
- [ADR-011](ADR-011-user-consent-grant.md) — OAuth 2.1 + PKCE that consumes G's scope catalog.
- [ADR-012](ADR-012-cross-tenant-dek-erasure.md) — dangling-reference resolver shape is part of D.
- [domain-model.md](../design/domain-model.md) — resources these conventions apply to; sensitivity tiers behind I.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 28 (true spec); 35–36 (one of each).
- OpenAPI 3.1 — <https://spec.openapis.org/oas/v3.1.0>; RFC 9457 — <https://www.rfc-editor.org/rfc/rfc9457>; RFC 3339 — <https://www.rfc-editor.org/rfc/rfc3339>.

## Notes

The load-bearing property is A: code → spec → clients with CI diffing the
committed spec against the emitted one. Every other convention is in service
of a true public contract. A future hand-edit of the committed spec is the
moment the discipline has failed — the fix belongs in the handler annotation.
