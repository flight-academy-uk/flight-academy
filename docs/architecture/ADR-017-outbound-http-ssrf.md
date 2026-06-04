# ADR-017 — Outbound HTTP and SSRF posture

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-03 |
| **Deciders** | @ICreateThunder |
| **Tags** | security, ssrf, defence-in-depth, network, integrations, webhooks |
| **Supersedes** | (none — refines [ADR-004](ADR-004-defence-in-depth.md), extends [ADR-001 §E](ADR-001-platform.md)) |

## Context

[ADR-004](ADR-004-defence-in-depth.md) commits to defence in depth on
inbound surfaces: rate limiting, billing-attack circuit breaker,
audit logging, deception. Outbound HTTP — wherever the platform
initiates a request on behalf of a user-controlled value — is the
symmetric gap. Classic SSRF (server-side request forgery) exfiltrates
cloud metadata, reaches internal services, or bypasses network
boundaries by tricking the server into making the request on the
attacker's behalf.

Outbound surfaces in the architecture:

- **Webhook delivery**
  ([ADR-009](ADR-009-event-streams-and-retention.md)) — tenant
  supplies the receiver URL; we POST signed events.
- **Tenant branding assets** — historically a runtime-fetch surface
  (tenant-supplied logo / font URLs). Now closed by upload-at-
  configuration per [ADR-014 §F](ADR-014-frontend-architecture.md);
  the outbound surface no longer exists.
- **Integration adapters** ([ADR-001 §E](ADR-001-platform.md),
  `flight-academy-integrations`) — outbound to Stripe, Xero, others.
  Provider URLs mostly fixed; tenant-configurable endpoints where
  applicable.
- **OAuth dynamic client registration**
  ([ADR-011 §D](ADR-011-user-consent-grant.md)) — if metadata-URL
  flow is ever added, that is SSRF surface; current form-based
  registration is not.
- **Future EFB data sources**
  ([ADR-016 §I](ADR-016-compliance-baseline.md)) — AIRAC, weather,
  NOTAM ingestion.
- **Forward — insurance evidence delivery**
  ([ADR-008](ADR-008-data-sharing-posture.md) Notes) — signed reports
  to insurer URLs.

Attack targets if unguarded: cloud metadata endpoints
(`169.254.169.254`), internal services (`localhost:5432`,
`localhost:6379`), other tenants' isolated services, DNS rebinding
(URL validates safe, fetches malicious at connect time).

Constraints: a single chokepoint so the policy is enforced once and
auditable, not scattered; defence in depth — application-layer
checks alone do not stop DNS rebinding without network-layer
policy; self-host parity (the application-layer protection ships
either way; network-layer is operator responsibility, documented).

## Decision

**Outbound HTTP passes through a single `OutboundHttpClient`
chokepoint enforcing scheme, IP, redirect, and timeout policy.
Kubernetes NetworkPolicy at the cluster layer independently denies
pod egress to private and metadata ranges. AWS IMDSv2 enforced on
EC2-backed nodes.**

### A. Single chokepoint

A single `OutboundHttpClient` lives in `flight-academy-integrations`
([ADR-005 §C](ADR-005-workspace-layout.md)). No new crate; the
chokepoint shares the home of the integration adapters that are its
primary consumers. Extracting a dedicated `flight-academy-egress`
crate is deferred per the [ADR-005 §F](ADR-005-workspace-layout.md)
extraction-trigger principle — done only when a second non-integration
consumer surfaces a real dependency-bundling cost.

The chokepoint is configurable per call-site with a small policy
enum — `Sensitive` (webhook delivery, OAuth callback), `Normal`
(integration adapter), `Permissive` (controlled fetch of
known-safe public asset). The default is `Sensitive`.

**Three-layer enforcement against bypass:** the discipline is
defended in depth so contributors cannot accidentally route around
the chokepoint:

- **Crate-level boundary (primary; compiler-enforced).** Application
  crates do not list `reqwest`, `hyper`, or `tokio::net::TcpStream`
  as direct dependencies. The chokepoint crate re-exports the
  necessary types via its public API. An accidental construction of
  `reqwest::Client` in `apps/api` fails to compile because the
  symbol isn't in scope. Zero runtime cost; deterministic.
- **`cargo deny` banned-types rule (backup).** `deny.toml` lists
  `reqwest::Client`, `reqwest::ClientBuilder`, `hyper::Client`, and
  similar as banned constructions outside `flight-academy-integrations`.
  Catches the case where someone adds a dependency that
  transitively pulls these types in.
- **CI grep sanity check (tripwire).** `rg
  "reqwest::Client::|reqwest::ClientBuilder|hyper::Client::"` against
  application-crate source paths fails CI if any match. Catches
  edge cases like comments referencing bypass patterns or imports
  via aliases.

Three layers, none expensive. Crate boundary is the workhorse;
`cargo deny` and grep are tripwires for the rare ways the boundary
could be circumvented.

### B. Application-layer checks

- **HTTPS only.** `http://` is rejected at the client. Exceptions
  require an explicit policy flag and an audit-chain entry; v1 has
  none.
- **IP denylist applied at hostname resolution.** Reject hostnames
  whose resolved A/AAAA records fall in RFC 1918
  (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), CGNAT
  (`100.64.0.0/10`), link-local (`169.254.0.0/16`), loopback
  (`127.0.0.0/8`, `::1`), multicast, broadcast, unique-local IPv6
  (`fc00::/7`), and IETF-reserved ranges. Both address families
  covered.
- **Re-resolve at connect time.** The first resolution may pass
  while the connect-time resolution returns a denied IP (DNS
  rebinding). The client re-resolves immediately before connect,
  validates the IP again, and **connects to the IP directly**, not
  the hostname. TLS SNI carries the original hostname for
  certificate validation.

### C. Redirect handling

- **`Sensitive` policy: no automatic redirect following.** The
  client surfaces the redirect response to the caller, which
  decides explicitly. Webhook delivery treats a redirect as
  receiver misconfiguration and records the response in the
  delivery log ([ADR-009](ADR-009-event-streams-and-retention.md)).
- **`Normal` policy: follow up to N redirects (default 3)** with
  the IP denylist re-applied at each hop and the scheme re-checked
  (`http://` after `https://` redirect rejected).
- **Open redirect** — server returning a redirect to a user-supplied
  URL — is the related browser-side concern. The only such surface
  is OAuth `redirect_uri`, allow-listed at client registration
  ([ADR-011 §D](ADR-011-user-consent-grant.md)). No general
  open-redirect endpoint exists.

### D. Bounded timeouts and resource limits

| Policy | Connect | Total | Body cap |
| --- | --- | --- | --- |
| `Sensitive` | 5s | 30s | 1 MiB |
| `Normal` | 5s | 60s | 10 MiB |
| `Permissive` | 10s | 300s | 100 MiB |

User-facing synchronous paths apply the strictest applicable
policy. A timeout records the outcome in the audit chain
([ADR-004 §D](ADR-004-defence-in-depth.md),
[ADR-009](ADR-009-event-streams-and-retention.md)) for
receiver-side diagnosis.

### E. Network-layer policy

Independent of the application-layer client; defence in depth.

- **Kubernetes NetworkPolicy** on application pods denies egress to
  RFC 1918, CGNAT, link-local, loopback, multicast, broadcast, and
  the IPv4 metadata range (`169.254.169.254/32`).
- **AWS IMDSv2 enforced** on EC2-backed nodes. Node-launch
  configuration disables IMDSv1. IMDSv2's session-token requirement
  closes the classic SSRF-to-credentials chain if the
  application-layer check is bypassed.
- **No host-network pods** in workload namespaces; only kube-system
  pods that legitimately need it. Admission-control rejection at
  the cluster level.
- **Egress proxy** (e.g. squid with allow-list) is **not** required
  at v1; the application-layer chokepoint plus NetworkPolicy is
  sufficient. A future deployment posture may add one if the
  outbound surface grows significantly.
- **Cloud-specific magic IPs beyond link-local.** The IPv4 link-local
  denylist (`169.254.169.254/32`) covers the metadata endpoint
  convention adopted by AWS, GCP, Azure, DigitalOcean, Oracle,
  Alibaba, and OpenStack — every cloud realistically on the roadmap.
  However, some platforms expose additional internal endpoints in
  *public* IP ranges that are not caught by the link-local rule.
  Notably: **Azure's Wire Server / Fabric / DNS endpoint at
  `168.63.129.16`** sits in Microsoft-allocated public space and
  serves guest-agent, platform-DNS, and health-probe traffic. Any
  new cloud target entering the deployment matrix
  ([ADR-002](ADR-002-release-deployment.md)) must audit for and
  explicitly denylist its equivalents before workloads land. Bare
  metal and self-host deployments have no such endpoints.

### F. Surface map

| Surface | Policy | Notes |
| --- | --- | --- |
| Webhook delivery ([ADR-009](ADR-009-event-streams-and-retention.md)) | `Sensitive` | No redirect follow; tenant-supplied URL; receiver verifies signature ([ADR-013 §C](ADR-013-auth-keys.md)) |
| Tenant branding assets (logos, fonts) | **Not applicable — uploaded** | Tenants upload assets at configuration time per [ADR-014 §F](ADR-014-frontend-architecture.md); no runtime fetch of tenant-supplied URLs |
| Integration adapter (Stripe, Xero, …) | `Normal` | Mostly fixed provider URLs; per-adapter configuration validated at startup |
| OAuth client domain verification at promotion to `verified` ([ADR-011 §D](ADR-011-user-consent-grant.md)) | `Sensitive` | Fetch of a verification token from the developer's homepage URL during promotion; one-shot per promotion, routed through the chokepoint with full IP denylist + redirect refusal |
| OAuth dynamic *metadata-URL* registration (RFC 7591 style) | `Sensitive` | Not implemented in v1; flagged for future. Current registration is form-based, not URL-fetched. |
| EFB data sources (forward — [ADR-016 §I](ADR-016-compliance-baseline.md)) | `Normal` | AIRAC / weather / NOTAM providers; AIRAC cycle discipline |
| Insurance evidence delivery (forward — [ADR-008](ADR-008-data-sharing-posture.md) Notes) | `Sensitive` | Insurer-supplied URLs |

### G. Self-host

The application-layer chokepoint ships with the tenant binary in any
deployment mode. The network-layer NetworkPolicy and IMDSv2
enforcement are the operator's responsibility on self-host —
documented in `docs/operations/self-host-conformance.md` (TBD). A
self-host deployment without these layers loses defence in depth but
retains the application-layer protection.

### H. Failure modes

- **DNS rebinding** — first resolution passes; attacker's DNS
  returns internal IP at connect time. Mitigation: re-resolve and
  validate at connect (§B).
- **IPv6 bypass** — IPv4-mapped IPv6 (`::ffff:10.0.0.1`) or
  unique-local IPv6 (`fc00::/7`) can bypass v4-only checks.
  Mitigation: denylist covers both address families.
- **Application-layer bypass** — direct `reqwest::Client` in
  application code skips the chokepoint. Mitigation: CI lint
  forbids direct construction in application crates (§A).
- **Network-layer bypass** — misconfigured NetworkPolicy or a
  host-network pod reaches denied ranges. Mitigation: CI validation
  of NetworkPolicy manifests; admission-control rejection of
  host-network pods outside kube-system.
- **Cloud-metadata via SDK** — application code using the AWS SDK
  legitimately reaches IMDS via the metadata client; the
  NetworkPolicy permits this from application pods (IRSA path).
  The denylist applies to *user-influenced* outbound HTTP;
  SDK-driven IMDS access is permitted by a separate egress rule
  scoped to AWS SDK call sites.

## Consequences

**Positive.** One chokepoint, one policy. Application-layer plus
network-layer is defence in depth; bypassing both is materially
harder than bypassing either. DNS rebinding closed by re-resolution.
AWS IMDS classic SSRF closed by IMDSv2. Self-host gets the
application-layer protection automatically; network-layer is
documented as operator responsibility. CI lint stops
direct-client regressions at PR time.

**Negative.** Real engineering surface: a chokepoint crate, lint
rule, NetworkPolicy manifests, admission-control checks.
Re-resolution adds ~1ms to outbound connect latency. The CI lint
forbidding direct HTTP clients is a real ergonomic constraint —
contributors writing integration code must use the chokepoint, not
`reqwest` directly.

**Neutral.** The chokepoint adds an indirection but the API is thin
(one struct, a `request` method, policy enum). Per-call-site policy
selection is a small ergonomic cost.

## Alternatives considered

- **Application-layer checks only, no NetworkPolicy.** Cheaper,
  single layer. Rejected: DNS rebinding and code-level bypass are
  real and the network-layer catches both. The marginal cost of
  NetworkPolicy is small.
- **NetworkPolicy only, no application-layer chokepoint.**
  Operator burden; less portable across cluster configurations;
  cannot enforce redirect or scheme checks. Rejected.
- **Egress proxy (squid / forward proxy with allow-list)** instead
  of the chokepoint. Higher operational cost; another component to
  monitor; harder to test locally; redirect handling moves to the
  proxy. Rejected at v1; reconsidered if outbound surface grows.
- **Per-call ad-hoc validation.** What
  [ADR-004](ADR-004-defence-in-depth.md)'s spirit likely assumed.
  Rejected: ad-hoc validation drifts; one chokepoint with a policy
  enum is auditable and uniform.

## References

- Refines [ADR-004 §A/§B/§E](ADR-004-defence-in-depth.md) —
  defence in depth extended to outbound HTTP; rate-limit philosophy
  generalised; deception layer's IP-based detection benefits from
  consistent outbound IP discipline.
- Extends [ADR-001 §E](ADR-001-platform.md) — integration
  adapters use the chokepoint by construction.
- [ADR-009](ADR-009-event-streams-and-retention.md) — webhook
  delivery routes through the chokepoint; failure modes recorded
  in the delivery log.
- [ADR-011 §D](ADR-011-user-consent-grant.md) — OAuth
  `redirect_uri` registration; dynamic client-metadata
  registration if ever added.
- [ADR-013 §C](ADR-013-auth-keys.md) — webhook receiver verifies
  against the per-tenant artefact key.
- [ADR-016 §E](ADR-016-compliance-baseline.md) — OWASP ASVS L2
  and NCSC CSP14 alignment for outbound HTTP discipline.
- OWASP SSRF Prevention Cheat Sheet —
  <https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html>.
- AWS IMDSv2 —
  <https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/configuring-IMDS-existing-instances.html>.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 28
  (truth — failure modes named honestly including IPv6-mapped bypass
  and Azure 168.63.129.16 gap), 34 (be not proud — Azure gap
  acknowledged rather than implicitly covered), 35–36 (restraint —
  single chokepoint, v1 omits egress proxy until trigger surfaces),
  38 (be not lazy — three-layer enforcement so contributors
  structurally cannot bypass; bypass discipline is compile-time, not
  procedural), 48 (watchfulness — defence-in-depth across application
  / network / cloud layers; cloud-specific magic IPs require explicit
  denylist per platform).

## Notes

The most reversible part of this ADR is the v1 omission of an
egress proxy. As the outbound surface grows (insurance, EFB
providers, more integrations), a forward proxy with explicit
allow-list becomes more attractive; the chokepoint architecture
already provides the indirection layer to plug a proxy in without
application changes.

The most load-bearing part is the chokepoint discipline — once
contributors learn to "just call `reqwest`" the SSRF surface
fragments. CI lint at PR time is what keeps this invariant.
