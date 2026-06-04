# ADR-013 — Auth keys and signing infrastructure

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-02 |
| **Deciders** | @ICreateThunder |
| **Tags** | auth, keys, jwt, jwks, kms, rotation, webhooks, signing |
| **Supersedes** | (none — refines [ADR-001 §F](ADR-001-platform.md), [ADR-011 §E](ADR-011-user-consent-grant.md)) |

## Context

[ADR-001 §F](ADR-001-platform.md) mentions a "tenant-aware signing key"
without specifying shape. [ADR-011 §E](ADR-011-user-consent-grant.md)
describes JWT access tokens (10-minute) and opaque rotating refresh
tokens without saying how the JWTs are signed or how keys rotate.
[ADR-009](ADR-009-event-streams-and-retention.md) mentions signed
webhook deliveries; [ADR-010](ADR-010-platform-operator-access.md)
introduces a staff plane with its own auth surface;
[ADR-012](ADR-012-cross-tenant-dek-erasure.md) establishes per-controller
DEKs for data encryption. None address signing key lifecycle.

Open questions: one signing key per tenant or one key with `tenant`
claim? Rotation cadence and overlap? JWKS endpoint shape and visibility?
KMS interaction across hosted and self-host? Distinct keys per artefact
class (session vs webhook vs consent grant)?

Forces: cryptographic blast-radius (compromise should bound damage);
rotation cadence (frequent enough to invalidate exfiltrated keys; not
so frequent it churns); JWKS endpoint cost (size, cache, list growth);
self-host parity (architecture must not require AWS KMS); audit clarity
(every key use traces to `kid`); external receiver verification (stable,
discoverable public keys).

## Decision

**Two key universes — session signing (per security plane, with `kid`
rotation) and artefact signing (per controller, lifecycle-tied to that
controller). Both use Ed25519. Session keys live in KMS; artefact keys
are envelope-wrapped under controller DEKs. JWKS published at
`/.well-known/jwks.json` carrying the rolling window of valid session
keys; per-tenant JWKS carries artefact public keys.**

### A. Two key universes

| Universe | Used for | Identity | Lifecycle | Storage |
| --- | --- | --- | --- | --- |
| **Session** | JWT access tokens ([ADR-011 §E](ADR-011-user-consent-grant.md)) for users and staff | One key per security plane, rotated by `kid` overlap | Monthly calendar rotation | KMS |
| **Artefact** | Webhook signatures ([ADR-009](ADR-009-event-streams-and-retention.md)), consent-grant assertions ([ADR-011](ADR-011-user-consent-grant.md)), transparency-report signatures ([ADR-010 §J](ADR-010-platform-operator-access.md)) | One key per controller, derived from controller DEK lineage ([ADR-012](ADR-012-cross-tenant-dek-erasure.md)) | Tied to controller existence — destroy DEK → destroy key | Envelope-wrapped under controller DEK |

Separation rationale: session keys serve high-volume, low-recipient-count
traffic (the API verifies its own tokens; JWKS is one endpoint). Artefact
keys serve low-volume, high-recipient-count traffic (external webhook
receivers verify long-lived signatures; signatures must outlive sessions).
Different cardinality, cadence, storage.

### B. Session signing — single key per plane with `kid` rotation

Two session keys exist:

- **Tenant API session key** — signs JWTs issued by the tenant binary
  (`apps/api`).
- **Staff plane session key** — signs JWTs issued by the staff binary
  ([ADR-010 §I](ADR-010-platform-operator-access.md), `apps/admin`).

They are not interchangeable. The tenant binary refuses staff-signed
JWTs; the staff binary refuses tenant-signed JWTs. Compromise of one
does not compromise the other.

Each key carries a `kid` in the JWT header. Verification looks up the
key by `kid` from a rolling window of valid keys (current + previous
during overlap).

**Tenant claim is in the JWT body, not the key.** A single tenant API
session key signs every tenant's JWTs; the JWT body claim `tenant_id`
carries authorisation context. Reasons:

1. Per-tenant signing keys would mean thousands of distinct KMS keys at
   scale (cost, cache, JWKS size).
2. RLS + ABAC enforce tenancy at the database layer; the JWT signature
   is not the tenancy-enforcement mechanism.
3. Tenant suspension is enforced via per-tenant revocation tables
   (checked on token use), not by key rotation.

**Trade-off:** a compromised tenant API session key forges JWTs for any
tenant. Mitigation: short token lifetime (10 minutes per
[ADR-011 §E](ADR-011-user-consent-grant.md)), rapid rotation capability
(rotate inside an hour if compromise suspected), and database-side ABAC
and RLS as substantive defence — a forged JWT remains subject to those.

### C. Artefact signing — per-controller, lifecycle-tied

Webhook signatures, consent grants, and tenant transparency reports
need recipient verification days or months after issuance. This argues
for:

1. **Stable per-recipient keys** — receivers fetch the public key once
   and cache it.
2. **Cryptographic tie to crypto-shred** — if the tenant exercises
   right-to-erasure or revokes a grant, the artefact key destroys with
   the DEK ([ADR-012](ADR-012-cross-tenant-dek-erasure.md)), making
   historical signatures permanently unverifiable. This is a feature:
   erasure is provable.

Per-controller key shape:

- **Eager generation at controller creation** (tenant signup, consent
  grant issuance, transparency-report dispatcher init). The KMS
  round-trip + envelope-wrap (~50ms) is paid at a rare event; signing
  latency on first use is deterministic; the invariant becomes
  "every controller row has a wrapped artefact key," which simplifies
  reasoning and eliminates double-checked-locking on first signing.
- Wrapped under the controller's DEK — unrecoverable once the DEK is
  destroyed.
- Stored with controller metadata in `flight-academy-db`.
- Public key exposed via the controller's JWKS endpoint (e.g.
  `tenants/{tenant}/.well-known/jwks.json`).

### D. JWKS publication

Three endpoint shapes:

| Endpoint | Carries | Audience | Cache |
| --- | --- | --- | --- |
| `/.well-known/jwks.json` (tenant binary) | Tenant API session key — current + previous | Internal verifiers; OAuth clients per [ADR-011](ADR-011-user-consent-grant.md) | `Cache-Control: max-age=600` |
| `/.well-known/jwks.json` (staff binary, internal DNS only) | Staff session key — current + previous | Internal staff-plane services | `Cache-Control: max-age=600` |
| `tenants/{tenant}/.well-known/jwks.json` (tenant binary) | Per-tenant artefact key(s) — current + retired-but-not-shredded | External webhook receivers, consent-grant verifiers | `Cache-Control: max-age=3600` |

Per-tenant JWKS lists the current artefact key plus any retired keys
not yet DEK-shredded, so verifiers validate signatures from recently
rotated material while receivers catch up.

### E. KMS interaction

| Environment | KMS | How |
| --- | --- | --- |
| **Hosted (AWS)** | AWS KMS via IRSA | Session keys KMS-resident; sign operations cross the KMS boundary (~10ms); verifies are local |
| **Hosted (other cloud)** | GCP KMS / Azure Key Vault | Same pattern, abstracted behind `KmsClient` trait in `flight-academy-auth` |
| **Self-host (full)** | `age`-encrypted key file unsealed at startup by ESO-equivalent | Matches the project's SOPS/`age` infrastructure pattern |
| **Self-host (minimal)** | In-process key derivation from operator master secret | For docker-compose without ESO; operator supplies 32-byte master via env var; session and artefact keys derived via HKDF |

KMS interaction is signing only — private keys never leave KMS in the
hosted case. The `KmsClient` trait covers `sign(kid, payload) -> Sig`
and `rotate() -> new_kid`. Verification is local (public keys cached).

### F. Rotation policy

| Key | Cadence | Overlap | Compromise response |
| --- | --- | --- | --- |
| Tenant API session key | Monthly | 30 days (previous `kid` verify-only) | Emergency rotate (≤1 hour); previous immediately invalid; sessions force re-auth |
| Staff plane session key | Monthly | 30 days | Emergency rotate; staff sessions force re-auth |
| Per-controller artefact key | Annual (or per-tenant choice) | 90 days for retired keys in JWKS | Emergency rotate; old signatures remain valid in JWKS until shredded |

Calendar rotation is the safe default. Operators may rotate sooner
without notice. Rotation is recorded in the platform audit chain
([ADR-004 §D](ADR-004-defence-in-depth.md),
[ADR-010 §E](ADR-010-platform-operator-access.md)) as an elevated
action.

### G. Self-host

A self-host deployment runs the tenant binary only
([ADR-010 §H](ADR-010-platform-operator-access.md),
[ADR-005 §F](ADR-005-workspace-layout.md)) and needs only:

- Tenant API session key (single).
- Per-controller artefact keys (one per tenant — usually one).

The staff plane session key does not exist on self-host. Rotation, KMS
interaction, and JWKS are all simpler. The minimal pattern (operator
master + HKDF) targets docker-compose installs without KMS.

### H. Failure modes

- **Session key compromise.** Emergency rotate; previous `kid`
  immediately invalidated; sessions force re-auth; platform-chain
  records the incident; tenant transparency notifications fire
  ([ADR-010 §J](ADR-010-platform-operator-access.md)).
- **Lost session key (KMS unavailable / deleted).** Active sessions
  invalidated; users re-auth via passkey/magic-link. Service
  degradation, not data loss.
- **Lost artefact key (DEK destroyed prematurely).** Historical
  signatures permanently unverifiable; feature for crypto-shred, flaw
  if accidental. Mitigation: DEK destruction requires confirmation
  ceremony and platform-chain entry.
- **KMS outage.** Sign operations stall; existing sessions remain valid
  until expiry; verification is local. Graceful degradation — new
  logins fail until KMS returns.
- **JWKS endpoint compromise.** Attacker substitutes their public key.
  Mitigation: external verifiers should pin keys for high-value
  artefacts; the API binary self-verifies via internal cache, not JWKS.

### I. Post-quantum readiness

Current decisions use Ed25519 (signatures) and KMS-provider-native
wrapping (envelope encryption). The post-quantum threat model is
asymmetric across primitives:

- **Session JWTs** (Ed25519, 10-min TTL): quantum risk is bounded by
  rotation cadence. A cryptographically-relevant quantum computer
  (CRQC) in N years can forge tokens valid in that window only. The
  operational cost of migrating to ML-DSA (~50x JWT-header size,
  ~3.3 KB per token) is not justified by the threat. Sessions stay
  Ed25519 through the foreseeable transition.
- **Artefact signatures** (Ed25519, lifetime years): quantum risk is
  real — a future CRQC could forge historical webhook events and
  consent grants. Migration target is **hybrid (Ed25519 + ML-DSA)**
  when (a) Rust ML-DSA crates (`fips204`, `pqcrypto-dilithium`) reach
  v1.0 and audit-grade maturity, and (b) KMS providers expose
  ML-DSA signing. Realistic timeline 2027-2030.
- **Envelope encryption** (AES-256 DEK, KMS-wrapped): AES-256 is
  PQ-safe under Grover (effective 128-bit, still strong). The
  wrapping mechanism is the migration point. **Inherited from KMS
  provider** as ML-KEM (FIPS 203) wrapping rolls out (AWS KMS GA in
  selected regions 2025; GCP KMS and Azure following). No application
  code change required when the provider migrates default.

Tracking: `docs/operations/pq-migration.md` (TBD) records primitives
in use, library/provider readiness, planned migration windows. The
migration posture is conservative — hybrid first, classical retired
only after PQ verifiers are universally deployed.

Self-host caveat: `age` (Curve25519-based) used for self-host key
sealing is **not** PQ-safe. `age` PQ support is in upstream discussion;
self-host PQ migration depends on that landing.

### J. Cipher-suite agility

Concrete primitives (Ed25519 today; ML-DSA or hybrid future per §I)
are abstracted behind `Signer` and `Verifier` traits in
`flight-academy-auth`. Call sites depend on the trait; compile-time
swap of the implementation requires no application changes.

JWS verification is **strict by construction**, not by configuration:

- `alg` is validated against a fixed allow-list (current + transition);
  algorithms outside the list are rejected at parse time including
  `alg=none`.
- Algorithm is looked up by `kid`, **not trusted from the JWS `alg`
  header**. The verifier dispatches from the stored key record's
  algorithm field; the JWS `alg` claim is cross-checked but never
  authoritative. This closes the JOSE family of algorithm-confusion
  attacks (HMAC-with-RSA-pubkey, `alg=none`).
- `kid` encodes algorithm context
  (`tenant_T-rotation_N-{alg}`) so the dispatch is unambiguous.

JWKS supports multiple key types simultaneously by JWK-native
`kty`/`alg` discrimination; the same controller publishing Ed25519 and
ML-DSA keys during a transition window is a JWKS-format operation,
not a protocol extension.

Hybrid signatures (Ed25519 + ML-DSA) compose at the `Signer` layer —
verifiers in transition accept either; verifiers in steady-state
hybrid require both. The migration path is documented in
`docs/operations/pq-migration.md` (TBD).

## Consequences

**Positive.** Two clear universes match two clear use cases (sessions =
short-lived, high-volume, one audience; artefacts = long-lived,
low-volume, many audiences). Per-controller artefact keys give
cryptographic erasure for free. Session-key rotation is cheap; artefact
rotation rare. Self-host pattern works without KMS dependence. Plane
separation extends naturally to key separation.

**Negative.** Single tenant API session key — compromise forges any
tenant's JWT, mitigated by short TTL, rapid rotation, and
database-layer enforcement, but real. Per-controller artefact keys
multiply key material with tenant count — modest cost. Three JWKS
endpoint shapes to maintain. KMS interaction adds ~10ms to every sign.

**Neutral.** Ed25519 over RSA for compactness and forward safety;
monthly rotation is calendar discipline, not technical pressure; the
`kid` header convention is standard JWS.

## Alternatives considered

- **Per-tenant session keys.** Cleaner blast-radius isolation.
  Rejected: KMS cost scales with tenant count; JWKS grows; tenant
  suspension already enforceable via session revocation tables.
- **Symmetric (HMAC) session signing.** Smaller, faster, no KMS
  round-trip. Rejected: same key signs and verifies; OAuth third-party
  clients can't verify without the secret; precludes external JWKS
  publication.
- **One key universe for everything.** Single key for sessions +
  webhooks + grants + reports. Rejected: erasure semantics break
  (destroying tenant key kills sessions too); webhook keys want years
  of stability, session keys want monthly rotation.
- **RSA over Ed25519.** Mature, ubiquitous. Rejected: larger keys,
  larger signatures, slower verify; no toolchain requirement.
- **No rotation, manual on incident.** Simpler. Rejected: rotation as
  exception is operationally fragile; calendar discipline catches both
  compromise and complacency.

## References

- [ADR-001 §F](ADR-001-platform.md) — refined here (signing-key
  ambiguity resolved).
- [ADR-009](ADR-009-event-streams-and-retention.md) — webhook
  signatures use artefact keys.
- [ADR-010 §E/§I/§J](ADR-010-platform-operator-access.md) — staff plane
  session-key separation; transparency reports signed with artefact
  keys.
- [ADR-011 §E](ADR-011-user-consent-grant.md) — JWT access tokens use
  session keys; consent-grant assertions use artefact keys.
- [ADR-012](ADR-012-cross-tenant-dek-erasure.md) — artefact keys
  wrapped under controller DEKs; erasure semantics inherit.
- [ADR-016 §B/§E](ADR-016-compliance-baseline.md) — UK GDPR
  right-to-erasure; NIST 800-63B AAL2/AAL3.
- RFC 7515 (JWS), 7517 (JWK), 7519 (JWT), 8037 (Ed25519 for JOSE).
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 24 (no
  pretence — PQ readiness honest about what's defended at each TTL),
  28 (truth — every key use traces to a `kid`, rotation audited, no
  shadow keys), 35–36 (restraint — two key universes, single tenant
  API session key, monthly rotation cadence), 38 (be not lazy —
  eager artefact-key generation, ≤1-hour emergency rotate, strict
  `alg` allow-list rather than trust-the-claim), 48 (watchfulness —
  failure modes enumerated; compromise response documented; PQ
  tracked).

## Notes

The single-tenant-API-session-key choice is the most reversible part of
this ADR. Should compromise patterns prove per-tenant keys are worth
the cost, the rotation infrastructure already supports it — `kid`
becomes `{tenant_id}-{rotation_n}`.

Artefact-key-per-controller is more load-bearing: the crypto-shred-via-
DEK property depends on it. Switching to a shared artefact key would
lose the cryptographic erasure guarantee that
[ADR-012 §A](ADR-012-cross-tenant-dek-erasure.md) relies on.
