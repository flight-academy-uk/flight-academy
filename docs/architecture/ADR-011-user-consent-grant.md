# ADR-011 — User-consent grant flow (OAuth 2.1) for user-owned data

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-29 |
| **Deciders** | @ICreateThunder |
| **Tags** | auth, oauth, user-consent, user-owned-data, scopes, sdk |
| **Supersedes** | (none — extends [ADR-001 §F](ADR-001-platform.md)) |

## Context

[ADR-008 §B](ADR-008-data-sharing-posture.md) classifies a member's personal
data (logbook, competency, CV, medical references) as **user-owned**,
reachable through the API only with the owning user's consent. ADR-008 §C
stated the rule (user-owned scopes ride only user-authorised credentials)
and deferred the actual flow here. Without it, user-owned data is reachable
only by first-party clients holding the user's session — the public API is
tenant-data-only.

Concrete cases: a pilot importing their logbook into a personal analysis
tool; a pilot's employer HR system reading hours-flown with the pilot's
consent; a pilot publishing their verifiable Pilot CV on a job board; an
ATO reading a *member's* logbook for currency tracking (the member must
consent — the tenant cannot self-authorise).

What this builds on: [ADR-001 §F](ADR-001-platform.md) passwordless +
short-lived JWT + opaque refresh (we reuse the primitives);
[ADR-006 §G](ADR-006-api-contract.md) tenant-scoped `fa_sk_…` keys (the
user-scoped counterpart here, deliberately separate); ADR-008 posture (the
frame); [ADR-004 §C](ADR-004-defence-in-depth.md) and
[ADR-009](ADR-009-event-streams-and-retention.md) audit machinery.

Forces: user control (GDPR Art. 7; instrument 28 — the user can verify
they're in control); standards (off-the-shelf libraries; inventing our own is
hostile); special-category absolute exclusion ([ADR-008 §B](ADR-008-data-sharing-posture.md));
restraint (smallest workable authorisation server — no Auth0/Keycloak);
self-host parity (same binary as the API, [ADR-002 §H](ADR-002-release-deployment.md)).

## Decision

**Third-party access to user-owned data is granted by the user via the
OAuth 2.1 authorisation-code flow with mandatory PKCE; scopes are a
user-consent-graded subset that can never carry special-category data; access
tokens are short-lived JWTs and refresh tokens are opaque, rotated, and
individually revocable; every grant and revocation is audited; the
third-party app is a GDPR Art. 28 processor under a DPA.**

### A. Protocol — OAuth 2.1 + PKCE (mandatory for every client)

[OAuth 2.1](https://datatracker.ietf.org/doc/draft-ietf-oauth-v2-1/) +
[PKCE (RFC 7636)](https://www.rfc-editor.org/rfc/rfc7636) required on every
client, public and confidential. 2.1 removes obsolete grants (implicit,
password) and mandates PKCE / exact-match redirect URIs / refresh-token
rotation — exactly the hardening we'd otherwise call out as house rules.

Only the **authorisation-code** grant is enabled in v1.

- *Implicit* — removed in 2.1.
- *Resource-owner password credentials* — incompatible with passwordless.
- *Client credentials* — that's tenant-scoped `fa_sk_…` keys
  ([ADR-006 §G](ADR-006-api-contract.md)); user-consent is by definition
  user-mediated.
- *Device authorisation* — deferred until a concrete need (e.g. a CLI client).

DPoP ([RFC 9449](https://www.rfc-editor.org/rfc/rfc9449)) and Rich
Authorization Requests ([RFC 9396](https://www.rfc-editor.org/rfc/rfc9396))
are **deferred** — plain bearer over TLS is acceptable for v1 given short
lifetimes and audit-rich revocation.

### B. Identity model — user-authorised, never tenant-issued

A grant authenticates *the user* and authorises *a client* on their behalf,
**per (user, client)**:

```text
grants (
  grant_id   uuid pk,
  user_id    uuid not null,
  client_id  text not null,
  scopes     text[] not null,
  granted_at timestamptz not null,
  revoked_at timestamptz,
  ...audit fields...
)
```

A tenant cannot grant on a user's behalf; a tenant-issued `fa_sk_…` key
cannot carry a user-grant scope ([ADR-008 §C](ADR-008-data-sharing-posture.md)).
The two credential types are **structurally distinguishable** at the token
layer: a user-grant access token carries `grant_id` in its claims; a tenant
key resolves to a Subject without one. Handlers serving user-owned data
require the `grant_id` claim and check the grant is still active.

### C. Scopes — user-owned data only, never special-category

Scopes are `me:<resource>:<action>`, mirroring `/me/…`
([domain-model §2.1](../design/domain-model.md)). Representative v1 set:

| Scope | Grants |
| --- | --- |
| `me:logbook:read` | Read flight log entries |
| `me:logbook:write` | Create/append flight log entries |
| `me:competency:read` | Read the competency profile |
| `me:cv:read` | Read the pilot CV (verifiable credential, §2.8) |
| `me:bookings:read` | Read the user's bookings across orgs |

Hard rules:

- **No scope grants special-category data.** No `me:medical:detail:*`, no
  reach to safety-reporter identity. The data-class rule
  ([ADR-008 §B](ADR-008-data-sharing-posture.md)) holds absolutely — a user
  cannot consent to what we promised not to serve.
- **`me:medical:status:read`** (returns *"medical Class N valid until D"*
  without certificate detail) is the on-ethos shape, aligning with the
  privacy-preserving evidence path in
  [ADR-008](ADR-008-data-sharing-posture.md) Notes. Available if a need
  arises; not enabled by default.
- **No coarse `me:read`.** Apps declare exactly the resources they need.
- **Tenant resources aren't user-grant scopes** — those use the tenant-key
  path ([ADR-006 §G](ADR-006-api-contract.md)) with the tenant admin's
  consent.

### D. Application registration

Developers register apps in the developer portal
([domain-model §2.13](../design/domain-model.md)), under a dedicated
**OAuth Clients** section separate from tenant API-key management. A
developer's own list of registered apps lives at `me/developer-apps`.
Registration produces:

| Field | Notes |
| --- | --- |
| `client_id` | public; identifies the app |
| `client_secret` | issued **only for confidential clients**; never for SPA/mobile/CLI |
| `client_type` | `confidential` or `public` |
| `lifecycle_stage` | `development` / `submitted` / `verified` — default `development`; controls authorisation set, consent-screen treatment, and scope availability (see "Client lifecycle stages" below) |
| `redirect_uris` | exact-match list; HTTPS only (`http://localhost` for dev) |
| `requested_scopes` | the maximal scope set the app may ever request; **scopes available depend on `lifecycle_stage`** |
| `test_users` | for `development` / `submitted`: explicit list of user ids permitted to authorise the app (cap ~25); ignored for `verified` |
| `application_metadata` | first-party hosted assets (logo, name, description) + homepage URL + support email + privacy policy URL + DPA acknowledgement — see "First-party hosting" below |

Public clients (SPA, mobile) hold no secret; PKCE binds the code. Confidential
clients additionally authenticate at the token endpoint. Apps that haven't
signed the DPA or whose metadata is incomplete cannot register.

**First-party hosting of consent assets.** Logo, name, and description are
**uploaded and stored by us, never referenced by URL** — closing SSRF at
the consent surface, XSS at the highest-trust UI, post-registration logo
swaps for phishing, user-IP leakage to third parties on consent views,
and asset unavailability when third-party servers go down:

- **Logo** — uploaded image, validated (PNG/JPEG/SVG, size, dimensions,
  content scan), stored in `flight-academy-store`
  ([ADR-001 §D](ADR-001-platform.md)), served from our origin.
- **Name** — plain text, length-capped, validated against an
  impersonation policy (no "Flight Academy", well-known third-party
  brands; no zero-width or Unicode-confusable characters), stored in
  our DB and escaped on render.
- **Description** — same treatment as name.
- **Homepage URL** — stored as a clickable link, **never fetched
  server-side at consent time**; rendered with
  `rel="noopener noreferrer"` and an "External site" affordance. The
  domain-verification step at promotion to `verified` (see lifecycle
  stages) is the only time the URL is fetched, and it routes through
  the [ADR-017](ADR-017-outbound-http-ssrf.md) chokepoint.

**Client lifecycle stages.** Three stages with different distribution
rights:

| Stage | Authorisation set | Consent screen | Scope availability |
| --- | --- | --- | --- |
| **Development** (default at registration) | Allow-listed `test_users` only (developer-added; cap ~25) | Prominent "Unverified app" warning + developer identity | Sensitive scopes unavailable (e.g. `me:medical:*` if ever enabled) |
| **Submitted for review** | Same as Development | Same | Same; review queue surfaces to platform staff ([ADR-010 §I](ADR-010-platform-operator-access.md)) |
| **Verified** | Any user | No warning; verified badge | All `requested_scopes` available subject to per-scope review |

Promotion to `verified` requires: developer identity verification (sole
trader or company); privacy policy + ToS URL review; **domain
verification** of the homepage URL (token-based, fetched through the
[ADR-017](ADR-017-outbound-http-ssrf.md) chokepoint); security
questionnaire for sensitive-scope apps. Failure to maintain verification
(privacy policy returns 404, domain expires, DPA lapses) returns the app
to `development` with existing grants frozen until restored.

**Registration UX — friction by design.** Multi-step flow framed
explicitly as "becoming a developer": accept developer terms (separate
from user terms); provide app details; upload first-party assets per
above; add `test_users` for Development stage (or confirm personal-use
only); confirm to receive `client_id` (+ `client_secret` for
confidential clients). Email verification required at registration;
identity verification required at promotion to `verified`. The friction
is deliberate — it raises the cost of registering a malicious app
specifically to mislead users into granting scopes.

**`client_secret` rotation (confidential clients).** Developers issue a new
secret in the portal; both old and new are valid for **30 days** to allow a
zero-downtime rollover, then the old secret expires. Rotation is audited
(H); a compromised secret can be revoked immediately (instant expiry of the
old, new issued in the same step).

**Scope-upgrade flow.** A registered app that needs new scopes after
release updates `requested_scopes` in the portal. **Existing grants do not
silently widen** — they continue to carry the scope set they were granted.
Users re-consent on the next authorisation-code flow, which now requests the
wider set; the consent screen highlights the newly-added scopes. An app may
choose to prompt re-consent earlier by initiating a fresh authorisation
flow; the user can decline and continue at the old scope set.

### E. Token model

Reusing [ADR-001 §F](ADR-001-platform.md):

| Token | Format | Lifetime | Storage (in the app) |
| --- | --- | --- | --- |
| **Access** | JWT (Ed25519); claims `sub`, `grant_id`, `client_id`, `scope`, `aud` | **10 min** | memory; never persisted |
| **Refresh** | opaque random 32 bytes | **30 days idle**, revocable | platform secure storage (Keychain, Credential Manager); confidential clients in encrypted server storage |

- **Refresh-token rotation** (OAuth 2.1): every use returns a new refresh +
  invalidates the old. **Reuse of an invalidated refresh** triggers
  immediate revocation of the entire grant (stolen refresh detected).
- **`aud`** is the API origin (`https://api.flight-academy.app`); validated
  per request.
- **Introspection** ([RFC 7662](https://www.rfc-editor.org/rfc/rfc7662))
  exposed for confidential clients without a JWT library; the JWT is
  self-contained for the common case.

### F. Consent UX — the user sees exactly what they're granting

The authorisation endpoint shows: application name + logo + homepage with a
"registered to" line; each requested scope in plain language ("Read your
flight log") with the API name beneath; **what is *not* granted** —
medical detail, safety-reporter identity — visible so it's on the record
those categories were excluded by design; duration ("Active until you
revoke; refresh tokens valid for 30 days"); revocation pointer ("Settings →
Connected apps"); DPA acknowledgement (linked).

Consent is deliberate per-grant; "remember my consent" auto-approval is
**not** offered in v1. If the user is unauthenticated, the standard
passwordless login ([ADR-001 §F](ADR-001-platform.md)) runs first.

**Unverified-app treatment.** Apps in `development` or `submitted`
lifecycle stages (D) display a prominent warning banner at the top of
the consent screen — *"This app has not been verified — continue only
if you trust the developer"* — alongside the developer's identity (the
verified Flight Academy user who registered the app) so the authorising
user can recognise them. A one-click "Cancel" affordance is always
present. Sensitive scopes are not requestable from unverified apps and
therefore never appear on their consent screens. All consent-screen
assets are first-party hosted per D; no third-party content is fetched
at consent time.

### G. Revocation and visibility

Users manage active grants at `me/connected-apps`: view client + scopes +
granted-at + last-used; revoke immediately. Revocation invalidates the
refresh token and breaks the rotation chain at the DB; access tokens
still in their 10-minute window are honoured to expiry, **unless** the
revocation reason is `compromise`, in which case the API checks revocation
per request via a small hot-set cache.

The OAuth revocation endpoint
([RFC 7009](https://www.rfc-editor.org/rfc/rfc7009)) is exposed for the app
itself to revoke its own access (the polite case where a user uninstalls).

Tenant admins see which third-party apps a tenant's members have granted
access to **in aggregate** (count by app, not by member, no PII): a
transparency signal for the tenant without revealing individual choices to
employers.

### H. Audit and processor obligations

Per-user audit chain ([ADR-009 §C](ADR-009-event-streams-and-retention.md))
receives a row for:

- every grant (`grant.create` — actor = user, metadata = client_id, scope
  set, ip, user-agent);
- every revocation (with reason);
- every refresh-token rotation (one row per rotation, not per access);
- every `client_secret` rotation by a confidential client (D);
- every **reuse-of-rotated-refresh-token** — flagged security event; also
  fires to the auth-event stream
  ([ADR-004 §C](ADR-004-defence-in-depth.md));
- every read of user-owned data via a user-grant token (per
  [ADR-009 §B](ADR-009-event-streams-and-retention.md) — user-owned reads
  are above operational class).

A registered app is a **GDPR Art. 28 processor** under a DPA signed at
registration. The DPA terms require erasure propagation: on revoke or
Art. 17, the app deletes the user's data within **30 days**. Compliance is
contractual; the app carries the audit-proof obligation.

## Consequences

**Positive.** User control is real and visible — Art. 7 met by construction.
Standards-based surface; off-the-shelf libraries; no custom auth.
Special-category exclusion enforced at the scope catalogue — a user cannot
consent to what we don't serve. One token primitive shared with first-party
(ADR-001 §F). Refresh rotation + reuse detection turns a stolen refresh
into a high-signal incident, not a silent compromise. Self-host parity —
runs in the same binary.

**Negative.** Authorisation-server surface is non-trivial (authorise, token,
revoke, introspect, JWKS, consent UI, registration, rotation, audit hooks);
mitigated by adopting a mature Rust OAuth library (`oxide-auth`,
`axum-oidc`, or similar — confirmed in implementation). DPA + processor
management is ongoing editorial work. No "remember consent" — small
friction trade-off for clarity. Two auth surfaces (first-party passwordless,
authorisation server) sharing primitives but with separate UI flows.

**Neutral.** JWT chosen for in-process verification; introspection covers
the rest. No DPoP / RAR in v1 — plain bearer over TLS is fine for the
lifetimes. Per-user audit chain absorbs grant rows without changes.
Confidential vs public clients both supported; the difference is a
`client_secret`.

## Alternatives considered

- **OAuth 2.0 (without 2.1 hardening).** Still has implicit + ROPC in its
  spec surface; policy via documentation is weaker than policy via "the
  protocol doesn't include this."
- **Hosted SaaS (Auth0, WorkOS, Stytch).** Third party in the auth path
  violates [ADR-001 §H](ADR-001-platform.md) (no telemetry) and adds a
  sub-processor with deep access.
- **Keycloak / Hydra as a separate deployable.** Every self-hoster runs a
  second service. Reconsidered only if our needs materially outgrow the
  bounded set above.
- **User-scoped API keys instead of OAuth.** No per-app scoping (user
  copies one secret around), no per-app revocation, no consent receipt for
  GDPR.
- **Fold user-grant scopes into tenant API keys.** Tenant isn't the
  controller of the user's portable data; consent + revocation can't fairly
  live on the tenant credential ([ADR-008 §C](ADR-008-data-sharing-posture.md)).
- **Special-category accessible under a high-trust scope.** Rejected for
  the same reason as in [ADR-008 §B](ADR-008-data-sharing-posture.md) —
  the regulatory + just-culture risk dwarfs any integration value, even
  with user consent. The verifiable-credential path
  ([ADR-008](ADR-008-data-sharing-posture.md) Notes) is the on-ethos
  answer.

## References

- [ADR-001 §F/§H](ADR-001-platform.md) — passwordless sessions reused; no-telemetry constrains auth-server choice.
- [ADR-002 §H](ADR-002-release-deployment.md) — self-host parity (same binary).
- [ADR-004 §B/§C](ADR-004-defence-in-depth.md) — rate limit + no-PII; auth-event stream receives refresh-reuse signals.
- [ADR-005 §C](ADR-005-workspace-layout.md) — authorisation-server code in `flight-academy-auth`.
- [ADR-006 §G](ADR-006-api-contract.md) — tenant-scoped `fa_sk_…` keys; this ADR is the user-scoped counterpart.
- [ADR-008 §B/§C/§E](ADR-008-data-sharing-posture.md) — data classes hold against user-grant scopes (C); user-authorised credentials only (implemented here); export/erasure paths.
- [ADR-009 §B/§C](ADR-009-event-streams-and-retention.md) — user-owned reads audited; per-user chain absorbs grants/revocations/rotations.
- [ADR-017](ADR-017-outbound-http-ssrf.md) — outbound HTTP chokepoint covers the only consent-related fetches (domain verification at promotion to `verified`); consent screen itself fetches nothing.
- [domain-model §2.1 / §2.13 / §4](../design/domain-model.md) — `me/connected-apps` + `me/developer-apps`; developer portal hosts client registration; sensitivity tiers.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — 28 (truth); 35–36 (restraint); 48 (refresh-rotation reuse triggers alarm).
- OAuth 2.1 — <https://datatracker.ietf.org/doc/draft-ietf-oauth-v2-1/>; RFC 7636 PKCE; RFC 7009 Revocation; RFC 7662 Introspection; RFC 8414 AS Metadata; RFC 9396 RAR (deferred); RFC 9449 DPoP (deferred). GDPR Art. 7 + Art. 28 — <https://gdpr-info.eu/>.

## Notes

Load-bearing decision is C: the scope catalogue. OAuth 2.1 is just
OAuth 2.1; JWT + opaque refresh is ADR-001 §F's shape; but the **set of
named scopes** is where this ADR commits to what user-owned data the API
will *ever* serve to a third party. "No coarse `me:read`; no
special-category; named per resource" is the truth-and-restraint shape we
owe users.

If the developer portal ever drifts toward a Stripe-style "every resource
has a read scope and you can have them all," apply the
[ADR-008](ADR-008-data-sharing-posture.md) test again: *does this let data
escape our erasure/audit boundary beyond the minimum a real workflow needs?*
If yes, the scope doesn't ship in that form. The escape valve for "prove
without sharing" remains the verifiable-credential path
([ADR-008](ADR-008-data-sharing-posture.md) Notes), not a wider grant.
