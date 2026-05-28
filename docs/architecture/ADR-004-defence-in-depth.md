# ADR-004 — Defence in depth

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-28 |
| **Deciders** | @ICreateThunder |
| **Tags** | security, ddos, billing, audit, rate-limiting, hardening |
| **Supersedes** | (none) |

## Context

Flight Academy runs a hosted offering on a deliberately fixed-cost compute base (ARM K3s on AWS EC2, see [ADR-002](ADR-002-release-deployment.md)) and ships the same source as a self-hostable product under AGPL-3.0. Two properties of that posture shape this ADR:

- The cluster cost is bounded by default. The initial deployment caps compute to a fixed node count and per-node resource limits, so a flood of traffic does not spin up more nodes or, by itself, produce a runaway compute bill. A future hybrid model — cluster nodes plus Lambda for burst, or a deliberate node scale-out — is anticipated; any such variable-compute dimension must carry its own hard bounds (max node count, concurrency caps) and would be specified in a later ADR.
- The variable cost lives at the edge. CloudFront request charges, CloudFront data-transfer-out, WAF request evaluation, and any per-invocation services are the surfaces where an attacker can convert traffic into money spent. AWS Shield Standard absorbs L3/L4 volumetric DDoS at no charge, but it does not address L7 request-rate abuse, and L7 abuse is precisely what inflates the edge bill.


A solo maintainer cannot watch dashboards around the clock. Defences therefore have to be layered, mostly automatic, and bounded — they must degrade safely without a human in the loop, and they must not depend on telemetry leaving the operator's control (the no-phone-home commitment in [ADR-001 §H](ADR-001-platform.md) is non-negotiable and applies here too).

The regulated tenant base (UK CAA / EASA ATOs, Part 145 maintenance organisations, airfield operators) brings a second requirement: a tamper-evident record of privileged actions. Every ABAC authorisation decision ([ADR-001 §C](ADR-001-platform.md)) and every de-anonymisation of a safety reporter ([ADR-001 §G](ADR-001-platform.md)) must be recorded in a way a regulator can trust and the operator cannot quietly rewrite. That record sits in tension with GDPR's right to erasure, and the design has to hold both.

The constraints that bound the solution space:

- **Economic** — bound the edge bill automatically; never let request volume translate into an unbounded invoice.
- **Operational** — solo maintainer; defences must run unattended and fail safe.
- **Regulatory** — tamper-evident audit trail with defined retention; GDPR data-subject rights honoured without breaking the trail.
- **Ethical** — privacy by default, no surveillance, watchfulness as a standing discipline ([CODE_OF_ETHICS.md instrument 48](../../CODE_OF_ETHICS.md)). Logs must not become a covert PII store.
- **Transparency vs. operational security** — the project is public from commit one. Documenting that a defence *exists* is good practice; publishing its exact tuning hands an attacker a map. This ADR resolves that tension with a public/private split, set out below.

### Public/private split

This ADR is public. It documents the *categories* and *approaches* of the platform's defences. It deliberately does not enumerate the exploitable specifics — exact billing-alert thresholds, the full honeypot path list, circuit-breaker tuning constants. Those live in an operations runbook at `docs/operations/hardening.md`, which is not advertised and may be kept private to the hosted deployment. Where this document gives figures or paths, they are illustrative examples, marked as such, present to convey shape rather than to state live values. Telegraphing the precise configuration of a deception layer defeats the deception; stating that the layer exists does not.

## Decision

We adopt a defence-in-depth posture in six parts, labelled A–F: billing-attack defence with an automated edge circuit breaker (A); application-layer rate limiting with structured logging that carries no PII (B); authentication-specific security controls (C); a tamper-evident, append-only audit log distinct from operational logs (D); honeypots and deception with a public/private boundary (E); and a baseline hardening standard applied across HTTP, TLS, auth, sessions, API, secrets, Kubernetes, and egress (F).

The sub-decisions follow.

### A. Billing-attack defence — fixed-cost compute, bounded edge exposure

**Decision: the cluster cost is bounded by design in the initial deployment; the variable edge cost is bounded by a layered defence whose final layer is an automated circuit breaker that can take the public edge to a static maintenance page and notify the maintainer.**

The threat is economic, not availability-first: an attacker floods CloudFront and WAF with junk requests not to take the service down but to run up per-request and bandwidth charges — "denial of wallet." Shield Standard covers the volumetric (L3/L4) layer for free; it does nothing for high-volume well-formed L7 requests. The defence is layered so that each layer removes a large fraction of the cost before the next layer is reached.

**Layer 1 — WAF rate-based rules.** Per-IP rate-based rules in AWS WAF throttle a single source that exceeds a request-rate threshold within the evaluation window, returning a block before the request reaches an origin. This stops the cheap, single-source flood.

**Layer 2 — CloudFront cache strategy.** The hosted app is static-heavy (SvelteKit `adapter-static`, served from S3 via CloudFront — see [ADR-002 §G](ADR-002-release-deployment.md)). A flood aimed at static content hits the CloudFront cache, not the origin, so its marginal cost approaches zero. The cache discipline:

- Hash-suffixed build assets (`app-3f9c2a.js` and similar) are immutable and cached with a long TTL — `Cache-Control: public, max-age=31536000, immutable`.
- `index.html` and the SvelteKit entry are cached briefly so a release propagates quickly, accepting a small origin-fetch rate on those few paths.
- API responses are not cached at the edge by default; the API path is governed by WAF and the rate limiter (decision B), not by caching.

A flood that targets cached assets is therefore largely free to serve. A flood that targets the API is governed by layers 1, 4, and decision B.

**Layer 3 — AWS Budgets with escalating alert thresholds.** AWS Budgets watch daily and month-to-date spend and fire alerts at escalating thresholds. The figures below are **illustrative**, not the live configuration — the real thresholds live in `docs/operations/hardening.md`:

| Threshold (illustrative) | Daily spend (illustrative) | Action |
| --- | --- | --- |
| Notice | ~£X | Email/Slack notification to maintainer |
| Elevated | ~£2X | Notification + heightened-watch flag |
| Critical | ~£4X | Notification + arms the circuit breaker (decision below) |

The point is the *shape* — escalating bands, with the highest band wired to an automated action — not the numbers.

**Layer 4 — AWS Cost Anomaly Detection.** Cost Anomaly Detection (free, machine-learning-based) watches for spend that deviates from the learned baseline and surfaces anomalies that a fixed threshold would miss — for example a slow ramp that stays under the daily budget but is clearly abnormal in shape. It is a detection signal feeding the same notification path, not an enforcement layer.

**Layer 5 — the circuit breaker.** A Budget Action wired to the critical threshold (or a manual trigger) invokes a small Lambda — equivalently an SSM document — that:

1. Switches the public CloudFront distribution to a pre-staged static maintenance response: a `503 Service Unavailable` with a `Retry-After` header, served from a small object already in S3 so the breaker has no dependency on the running cluster.
2. Notifies the maintainer through the operator's existing channel (Slack via the notification path described in [ADR-002 §E](ADR-002-release-deployment.md)).
3. Records the action.

The breaker can be implemented either by swapping the distribution's origin / custom-error-response to the maintenance object, or by disabling the distribution outright; the maintenance-page approach is preferred because it returns a correct, polite `503 + Retry-After` to legitimate users and search crawlers rather than a connection failure. Re-enabling is a deliberate manual action by the maintainer after assessing the cause — the breaker does not auto-reset, on the principle that the cheaper failure is "service paused, maintainer paged" rather than "service flapping while the bill climbs."

Illustrative Terraform for the budget and the budget-action shape (values illustrative):

```hcl
# Illustrative — real thresholds live in docs/operations/hardening.md
resource "aws_budgets_budget" "edge_daily" {
  name         = "flight-academy-edge-daily"
  budget_type  = "COST"
  limit_amount = "50"        # illustrative
  limit_unit   = "GBP"
  time_unit    = "DAILY"

  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 100   # percent of limit — illustrative
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_sns_topic_arns  = [aws_sns_topic.budget_alerts.arn]
  }
}

resource "aws_budgets_budget_action" "edge_circuit_breaker" {
  budget_name        = aws_budgets_budget.edge_daily.name
  action_type        = "RUN_SSM_DOCUMENTS"   # or invoke a Lambda
  approval_model     = "AUTOMATIC"
  execution_role_arn = aws_iam_role.budget_action.arn

  action_threshold {
    action_threshold_value = 400   # percent — the "critical" band, illustrative
    action_threshold_type  = "PERCENTAGE"
  }

  definition {
    ssm_action_definition {
      action_sub_type = "STOP_EC2_INSTANCES"  # placeholder; real document swaps the
      region          = "eu-west-2"            # CloudFront origin to the S3 503 page
      instance_ids    = []
    }
  }

  subscriber {
    address           = aws_sns_topic.budget_alerts.arn
    subscription_type = "SNS"
  }
}
```

**Rationale and cross-reference.** This decision is the direct consequence of the fixed-cost-compute preference recorded in [ADR-001](ADR-001-platform.md) (Context, *Economic*) and the topology in [ADR-002 §G](ADR-002-release-deployment.md). The cluster bill cannot run away because the cluster does not scale per request. The remaining variable risk is at the edge, and the circuit breaker bounds the edge: in the worst case the public site serves a static `503` until a human looks at it, and the bill stops climbing. We deliberately accept "the service can be paused automatically" as the safe failure mode, because an unbounded invoice is a worse outcome for a solo-maintained project than a bounded outage.

### B. Application-layer rate limiting and structured logging

**Decision: WAF stops the bulk of abusive traffic at the edge; the application enforces a second, independent rate-limit layer with `tower_governor`, and emits structured JSON logs to stdout carrying request context but no PII.**

Defence in depth means not trusting a single control. WAF rate-based rules are the first cut, but they evaluate at the edge on coarse signals; the application has finer context (authenticated subject, tenant, endpoint cost) and can apply a second limit that survives even if a request reaches the origin via a path WAF did not catch. The `tower_governor` middleware (already listed in the [ADR-001](ADR-001-platform.md) tooling section) enforces per-key token-bucket limits in-process.

**Structured logging.** The application uses the `tracing` crate with a JSON formatter, one span per request, and `x-request-id` propagation (generated at ingress if absent, echoed on the response, threaded through every span). Each request emits a structured event with a fixed field set:

```rust
// Illustrative — field set, not final API
tracing::info!(
    request_id = %req_id,
    tenant_id = %tenant_id,
    user_id = %user_id,            // opaque UUID, never an email or name
    method = %method,
    path = %path,                  // route template, not raw query string
    status = status.as_u16(),
    latency_ms = latency.as_millis() as u64,
    source_ip = %client_ip,        // real client IP, validated below
    user_agent = %ua,
    "request completed"
);
```

**Real client IP.** Because CloudFront and the NLB sit in front of the application ([ADR-002 §G](ADR-002-release-deployment.md)), the source IP must be extracted from the trusted proxy chain rather than the immediate socket peer. The application trusts the forwarded client IP only from CloudFront; it does not accept an arbitrary `X-Forwarded-For` from an untrusted hop. This is what makes per-IP rate limiting and audit `source_ip` meaningful rather than spoofable. (It is also one of the reasons a third-party reverse proxy in front of CloudFront is rejected — see Alternatives.)

**No PII in logs — enforced, not aspirational.** Operational logs must never carry personal data. Email addresses, names, medical-certificate fields, and similar are blocked from log payloads: log-bearing types implement a redacting `Debug`/`Display`, identifiers are logged as opaque UUIDs, and paths are logged as route templates rather than raw URLs that might carry an identifier in a query string. This ties directly to the no-telemetry stance in [ADR-001 §H](ADR-001-platform.md) and to GDPR data minimisation — a log pipeline that accumulated PII would itself become regulated personal data and a breach liability.

**Where logs go.** Logs are written to stdout as JSON. The operator collects them via whatever pipeline they choose (Vector, Fluent Bit, journald shipping). There is no hardcoded log destination and no phone-home — consistent with [ADR-001 §H](ADR-001-platform.md). Self-hosted instances are observable only to their own operators.

### C. Authentication security controls

**Decision: authentication events are logged to a separate, longer-retained stream; failed attempts trigger an escalating backoff-then-lockout schedule per IP+user; and auth endpoints enforce a constant minimum response time to defeat username-enumeration timing attacks.**

This builds on the passwordless session design in [ADR-001 §F](ADR-001-platform.md) (magic link, passkeys/WebAuthn, push-to-paired-device, short-lived JWT + opaque revocable refresh token). Passwordless removes a class of attacks; it does not remove the need to defend the authentication surface itself.

**Auth-event logging.** Authentication events go to a stream separate from general operational logs and are retained longer, because they are the primary forensic record after a suspected account compromise. Each event carries `user_id`, `source_ip`, `user_agent`, an `event` kind, and a `reason`:

| Event | Notes |
| --- | --- |
| `login_success` | Which method (magic link / passkey / push) |
| `login_failure` | Reason category — never the supplied secret |
| `lockout` | Threshold reached; lockout duration |
| `password_not_applicable` | Passwordless flow; recorded for completeness |
| `passkey_added` | New WebAuthn credential registered |
| `session_expired` | Access or refresh token reached end of life |
| `refresh_revoked` | Refresh token explicitly revoked |

These events still carry no PII beyond the opaque `user_id` and the network metadata that is inherent to the event.

**Backoff and lockout.** Repeated failed attempts against the same IP+user pair trigger an escalating delay, then a temporary lockout, then a longer lockout with a maintainer alert. The schedule below is illustrative; exact values live in `docs/operations/hardening.md`:

| Failed attempts (illustrative) | Response |
| --- | --- |
| 1–3 | Normal processing |
| 4–6 | Escalating artificial delay added to the response |
| 7–9 | Temporary lockout of the IP+user pair (minutes) |
| 10+ | Extended lockout (hours) + auth-event `lockout` + maintainer alert |

Keying on IP+user (rather than user alone) avoids letting one attacker lock a victim out of their own account purely by guessing from elsewhere, while still throttling a focused attack from a single source.

**Constant minimum response time.** Auth endpoints enforce a floor on response time (on the order of ~200 ms) regardless of whether the supplied identifier corresponds to a real account. This defeats username/account-enumeration via timing side channels — an attacker cannot distinguish "no such user" from "user exists, auth failed" by measuring latency. The floor is applied uniformly; the work done to reach it is constant-time where it matters.

### D. Audit log — tamper-evident, append-only

**Decision: a dedicated append-only Postgres table `audit_events`, separate from operational logs, protected by row-level security and an INSERT-only grant, hash-chained for tamper evidence, and archived to S3 with Object Lock for compliance retention.**

The audit log answers a different question from operational logs. Operational logs answer "what was the system doing"; the audit log answers "who did what to which resource, and can we prove the record was not altered after the fact." A regulator auditing an ATO or a Part 145 organisation will want the latter. Mixing the two (see Alternatives) destroys the property that makes the audit log useful.

**Separation.** `audit_events` is a database table, not a log stream. It is queryable, constrained, and backed by the same durable, replicated Postgres (CNPG) that holds tenant data — not by a best-effort log pipeline that can drop records under load.

**Schema.** Illustrative DDL:

```sql
-- Illustrative — append-only audit trail
CREATE TABLE audit_events (
    id            BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    tenant_id     UUID,                       -- null for system-level events
    actor_id      UUID,                       -- the subject who acted
    actor_type    TEXT NOT NULL               -- 'user' | 'system' | 'api' | 'admin'
                  CHECK (actor_type IN ('user','system','api','admin')),
    action        TEXT NOT NULL,              -- e.g. 'authz.permit', 'safety.deanonymise'
    resource_type TEXT NOT NULL,
    resource_id   TEXT,
    metadata      JSONB NOT NULL DEFAULT '{}',-- decision rationale, etc. — no PII
    source_ip     INET,
    user_agent    TEXT,
    prev_hash     BYTEA,                      -- hash of the previous row (per chain)
    row_hash      BYTEA NOT NULL              -- hash over this row's canonical form
);

-- Tenants see only their own events.
ALTER TABLE audit_events ENABLE ROW LEVEL SECURITY;
CREATE POLICY audit_tenant_isolation ON audit_events
    USING (tenant_id = current_setting('app.current_tenant')::uuid);

-- The application role may INSERT and SELECT, never UPDATE or DELETE.
REVOKE ALL ON audit_events FROM app_api;
GRANT INSERT, SELECT ON audit_events TO app_api;
```

**Append-only enforcement.** The `app_api` role (the role the running API uses — see [ADR-002 §F](ADR-002-release-deployment.md) for the role separation) is granted `INSERT` and `SELECT` only. It holds no `UPDATE` or `DELETE` privilege on `audit_events`, so a compromised application cannot rewrite history. Schema-level DDL is the migrator role's province, not the API's. Row-level security scopes reads to the requesting tenant so one tenant cannot read another's audit trail.

**Hash chain for tamper evidence.** Each row stores `row_hash = H(canonical(row) || prev_hash)`, where `prev_hash` is the `row_hash` of the immediately preceding row in that chain. Altering or deleting any historical row breaks the chain from that point forward, and the break is detectable by re-walking the chain and recomputing hashes. The chain does not prevent tampering at the storage layer; it makes tampering *evident*, which combined with the INSERT-only grant and the S3 Object Lock archive (below) is the property regulators ask for. A periodic verification job re-walks recent chain segments and alerts on any mismatch.

**Archival and retention.** Audit rows are periodically archived to S3 with Object Lock in compliance mode, which prevents deletion or overwrite of the archived objects for the retention period even by an account administrator. This gives a write-once retention store independent of the live database. The working retention assumption is the 7 years noted as undecided in [ADR-001](ADR-001-platform.md) (Consequences, Neutral); this ADR adopts 7 years as the audit retention baseline, configurable per deployment, aligned with regulatory retention norms.

**GDPR tension — immutable audit vs. right to erasure.** An append-only, hash-chained, Object-Locked audit trail is, by design, hard to erase from. GDPR's right to erasure (Article 17) appears to demand the opposite. The two are reconciled by keeping PII *out* of the audit rows in the first place:

- Audit rows reference actors and resources by opaque UUID, never by name, email, or other direct identifier. `metadata` carries decision rationale and structured context, not personal data.
- When a data subject exercises erasure, the personal data they own is rendered unrecoverable by crypto-shredding the relevant DEK ([ADR-001 §D](ADR-001-platform.md)). The audit rows that *reference* a now-shredded subject continue to exist as `actor_id = <uuid>` with no way to resolve that UUID back to a person — the identifier becomes a dangling pseudonym. The chain stays intact; the person is no longer identifiable from it.
- Where an audit row would otherwise need to carry an identifier, it is pseudonymised at write time so that erasure of the subject does not require breaking the chain.

This means the right to erasure is satisfied (the person is no longer identifiable) without sacrificing the tamper-evidence property (no row is altered or removed). It is a deliberate design choice that the audit trail records *that an action occurred* permanently, while the *identifiability* of the actor is severable via crypto-shredding.

**What writes audit rows.** Every ABAC authorisation decision from [ADR-001 §C](ADR-001-platform.md) writes an audit row — every `Permit` with subject, action, resource and rationale; every `Deny` with the reason. Every de-anonymisation of a safety reporter ([ADR-001 §G](ADR-001-platform.md)) writes an audit row identifying the `safety_officer` who requested it, the occurrence, and the timestamp. Privileged administrative actions, key-unwrap operations, and configuration changes likewise write audit rows. The audit log is the single authoritative answer to "who did what."

### E. Honeypots and deception

**Decision: deploy several categories of honeypot and canary so that scanning and credential-misuse betray themselves early; document the categories publicly but keep the exploitable specifics — exact paths, exact identifiers — in the private operations runbook.**

Deception is most effective when the attacker does not know it is there. Publishing the precise configuration of a honeypot turns it into a list of things to avoid. We therefore document the *categories and approach* here and hold the operational specifics (the full path list, the exact canary identifiers, the auto-ban timings) in `docs/operations/hardening.md`.

**HTTP endpoint honeypots.** WAF rules match common scanner paths that no legitimate Flight Academy client would ever request — illustrative examples being `/.env` and `/wp-admin`. A request to such a path is a near-certain signal of automated scanning: it is blocked, the source is added to a temporary auto-ban, and the event is logged. The illustrative paths above convey the idea; the full list is in the private runbook precisely because publishing it would tell a scanner which paths to skip.

**Field-level honeypots.** Forms include hidden fields that a human user never sees and never fills. A submission with a non-empty honeypot field is a bot; the submission is rejected. This is a low-cost, high-signal filter against form-spamming automation that complements the rate limiter.

**Canary tokens.** Fake credentials and keys are planted where an attacker who gained read access would plausibly look (configuration, source, internal docs). They authorise nothing real; their only purpose is to alert if they are ever used, which indicates a leak or intrusion. AWS-key canaries are monitored via a CloudTrail listener for any use attempt; web canary tokens follow the canarytokens.org model (self-hosted where practical). A canary firing is treated as a high-priority incident signal.

**Honeypot tenant / user.** A tenant and user identifier exists that no legitimate party should ever access. Any authentication attempt against it, or any data access scoped to it, is by definition illegitimate and is treated as a breach signal — there is no false-positive path, because no real workflow touches it.

These deceptions telegraph the platform's defensive posture if detailed publicly, which is the whole reason for the public/private split described in the Context. The public record states they exist; the private runbook holds what would let an attacker route around them.

### F. Baseline hardening

**Decision: a baseline hardening standard is applied uniformly across the stack and treated as a regression-tested floor, not a one-time setup.**

| Area | Control |
| --- | --- |
| HTTP headers | `Content-Security-Policy` (strict, nonce-based); `Strict-Transport-Security` with preload (free via the `.app` TLD, which is HSTS-preloaded at the registry level); `X-Frame-Options: DENY`; `X-Content-Type-Options: nosniff`; `Referrer-Policy: strict-origin-when-cross-origin` |
| TLS | TLS 1.3 only; modern AEAD ciphers (ChaCha20-Poly1305, AES-GCM); CAA records pinning the permitted certificate authorities |
| Auth | Passkeys preferred; magic links short-expiry and single-use; all auth flows rate-limited (cross-ref [ADR-001 §F](ADR-001-platform.md), decision C above) |
| Sessions | Short-lived JWT access tokens; opaque revocable refresh tokens; concurrent-session limits per user |
| API | Pre-signed URLs for blob downloads — large objects are fetched directly from object storage by the client, never proxied through the application (bounds memory and bandwidth on the API tier) |
| Secrets | SOPS-encrypted in the infra repo, decrypted by Flux ([ADR-002 §A](ADR-002-release-deployment.md)); AWS Secrets Manager for production runtime secrets; never plaintext secrets in environment files committed anywhere |
| Kubernetes | `NetworkPolicy` default-deny with explicit allow rules; non-root containers; read-only root filesystem; drop all Linux capabilities. Enforcement layers: Cilium CNI for network policy, Istio ambient for mesh-level mTLS and L4/L7 policy, Tetragon for runtime intrusion detection |
| Egress | All outbound traffic via fck-nat (three fixed Elastic IPs, one per AZ — [ADR-002 §G](ADR-002-release-deployment.md)); default-deny egress `NetworkPolicy` with explicit allow rules; outbound webhooks HMAC-signed so receivers can verify origin |

The Kubernetes enforcement layers (Cilium, Istio ambient, Tetragon) deserve a note: network policy, mesh identity, and runtime IDS are complementary, not redundant. Cilium enforces what may talk to what at the packet level; Istio ambient enforces workload identity and mTLS; Tetragon observes syscall-level behaviour at runtime and can alert or block on anomalous process activity. Together they implement the "default-deny, then explicitly allow, then watch what actually happens" posture. This cross-references the deployment stack in [ADR-002 §G](ADR-002-release-deployment.md) (Istio ambient mesh) and the broader cluster tooling.

The fixed egress IPs serve a dual purpose already noted in [ADR-002 §G](ADR-002-release-deployment.md): tenant IT teams allow-list them for inbound webhook receipt, and HMAC signing lets those tenants verify that a webhook genuinely originated from Flight Academy rather than a spoofer who learned the IP.

## Consequences

### Positive

- **The edge bill cannot run away unattended.** Layered WAF + cache + budgets + anomaly detection + circuit breaker bound the variable cost. In the worst case the public site serves a static `503` and the maintainer is paged; the invoice stops climbing. This is the economic property the fixed-cost-compute decision in [ADR-001](ADR-001-platform.md) was chosen to enable.
- **Two independent rate-limit layers.** WAF at the edge and `tower_governor` in-process means no single bypass defeats throttling.
- **A regulator-grade audit trail.** Append-only, RLS-scoped, hash-chained, Object-Lock-archived — the operator can demonstrate who did what without being able to quietly rewrite it. ABAC decisions and safety-reporter de-anonymisations are permanently recorded.
- **Right to erasure and immutable audit coexist.** Crypto-shredding severs identifiability without breaking the chain; the design holds both regulatory requirements at once.
- **No PII leaks into logs or audit rows.** Both stores carry opaque identifiers only, keeping the no-surveillance commitment ([ADR-001 §H](ADR-001-platform.md)) honest and avoiding the creation of a covert personal-data store.
- **Early, high-signal intrusion indicators.** Honeypots and canaries fire on activity that has no legitimate explanation, giving a solo maintainer a credible early-warning system that needs no constant watching.
- **Username enumeration is closed off.** Constant-time auth responses remove the timing oracle.
- **Defences degrade safely and unattended.** The whole posture is designed to fail toward "paused and paged," appropriate to a solo-maintained project.

### Negative

- **The circuit breaker can pause the public service.** A false positive on the critical budget band — or a legitimate but unexpected traffic spike — can take the public edge to a `503` until a human re-enables it. We accept this; the alternative (an unbounded bill) is worse. The breaker does not auto-reset, so recovery requires maintainer action.
- **The audit log adds write cost and schema discipline.** Every privileged action writes a row, and the hash chain adds a dependency on the previous row's hash at write time, which serialises audit writes within a chain. At current scale this is acceptable; high-throughput audit workloads would need chain sharding.
- **Hash-chain verification is an ongoing job.** Tamper evidence is only as good as the verification cadence; the verification job is one more thing to run and monitor.
- **Honeypots require maintenance and carry a small false-positive risk.** Canary tokens must be rotated and watched; an honest misconfiguration that requests a honeypot path produces noise. The categories chosen minimise this, but it is non-zero.
- **The public/private documentation split is itself overhead.** Keeping `docs/operations/hardening.md` in sync with this public ADR, and deciding what is safe to publish, is continuing editorial work.
- **Real-client-IP extraction is fragile if the proxy chain changes.** The rate limiter and audit `source_ip` depend on trusting CloudFront's forwarded IP and nothing else; a topology change ([ADR-002 §G](ADR-002-release-deployment.md)) must update the trusted-hop configuration or the signals become spoofable.

### Neutral

- **Some defences are AWS-specific.** Budgets, Budget Actions, Cost Anomaly Detection, CloudFront, and S3 Object Lock are AWS services. Self-hosters on other clouds get the application-layer controls (rate limiting, audit log, honeypots, hardening) but must provide their own equivalents of the billing circuit breaker. This is documented for self-hosters; it is an accepted consequence of the hosted offering being AWS-first ([ADR-002 §G](ADR-002-release-deployment.md)).
- **Audit retention set to 7 years.** Adopted here as the baseline, configurable per deployment. This resolves the open question left in [ADR-001](ADR-001-platform.md) (Consequences, Neutral).
- **The maintenance page is pre-staged static content.** It must be kept current (correct branding, correct `Retry-After`) as a small ongoing task; in exchange the breaker has zero dependency on the running cluster.

## Alternatives considered

### Alternative — AWS Shield Advanced

Shield Advanced adds L7 DDoS protection, cost-protection credits, and a response team. Rejected at ~US$3,000/month as wildly disproportionate to the project's scale and budget. The combination of Shield Standard (free, L3/L4), WAF rate-based rules, a static-heavy CloudFront cache, and the billing circuit breaker covers the realistic threat for a fraction of the cost. Would reconsider only at a scale where the cost-protection credits and the response team's value clearly exceed the subscription — far beyond the current footprint.

### Alternative — Cloudflare proxy in front of CloudFront

Putting Cloudflare's proxy in front of CloudFront would add another L7 WAF and DDoS layer. Rejected for two concrete reasons. First, it conflicts with AWS WAF — running two L7 WAFs in series produces overlapping, hard-to-reason-about rule interactions and doubles the place a legitimate request can be wrongly blocked. Second, and decisively, it interposes a third party on the request path and breaks clean real-client-IP propagation (the IP the application sees becomes Cloudflare's, requiring trust of yet another forwarded-header hop), which undermines the per-IP rate limiting and audit `source_ip` in decisions B and D and sits poorly with the no-third-party-on-the-path posture in [ADR-002 §G](ADR-002-release-deployment.md) (where Cloudflare is explicitly used DNS-only, not proxied). Considered and rejected.

### Alternative — WAF only, no application-layer rate limiting

Relying solely on WAF rate-based rules would be simpler. Rejected because it is a single layer with coarse signals and no awareness of authenticated subject, tenant, or endpoint cost. A request that reaches the origin via a path WAF did not throttle would be unbounded. Defence in depth requires the second, in-process layer (decision B); the cost of `tower_governor` is negligible.

### Alternative — audit logs in the same table/stream as operational logs

Storing audit events in the operational log pipeline would save a table and a code path. Rejected because it destroys the properties that make an audit log useful: operational logs are best-effort (droppable under load), are not tamper-evident, are not RLS-scoped per tenant, and would, if they carried audit detail, become a place where the append-only and hash-chain guarantees could not hold. The audit log must be a constrained database table with its own grants and integrity chain, distinct from the log stream.

### Alternative — no honeypots (rely only on visible defences)

Running only the visible controls (WAF, rate limiting, hardening) avoids the maintenance and false-positive cost of deception. Rejected because honeypots and canaries are among the highest-signal, lowest-cost detection a solo maintainer can deploy: a hit on a honeypot path or a fired canary token is almost certainly malicious, where most other signals are ambiguous. Security through visibility alone forgoes that early warning. The public/private split mitigates the "telegraphing" downside.

### Alternative — third-party SaaS error aggregation (Sentry / Bugsnag / Rollbar)

A hosted error-aggregation service would give convenient crash and error dashboards. Rejected outright because it ships application data — potentially including request context — to a third party, which violates the no-telemetry / no-phone-home commitment in [ADR-001 §H](ADR-001-platform.md) and the no-error-aggregation statement in [SECURITY.md](../../SECURITY.md). Operators who want such a tool may wire up their own against the stdout JSON logs; the project ships none pre-wired.

## References

### Related ADRs

- [ADR-001 — Platform architecture](ADR-001-platform.md) — §C ABAC (audited here), §D envelope encryption / crypto-shredding (the erasure mechanism), §F passwordless sessions (built on by decision C), §G safety-reporter de-anonymisation (audited here), §H no telemetry (binds decisions B and the SaaS-error-aggregation rejection).
- [ADR-002 — Release and deployment](ADR-002-release-deployment.md) — §A SOPS-encrypted secrets, §F database role separation (`app_api`), §G CloudFront + WAF + NLB + Istio ambient topology and fck-nat fixed egress IPs that decisions A, B, D, and F align with.
- [ADR-003 — Database migration discipline](ADR-003-db-migrations.md) — the migrator-vs-API role separation that keeps `audit_events` DDL out of the API role's reach (forthcoming).

### Project documents

- [SECURITY.md](../../SECURITY.md) — threat model summary (multi-tenancy, credential compromise, supply chain, hostile hosting, operator error) that this ADR's controls implement; the no-telemetry and no-error-aggregation statements.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instrument 48 (watchfulness), "Keep constant guard over the actions of your life" — the standing instinct to ask "what does this look like to an attacker" that underlies the whole ADR; instruments 35–36 (restraint), applied to keeping the defence set as small as it can be while still doing its job.

### External standards and documentation

- RFC 9116 — `security.txt`, a machine-readable security-contact file at `/.well-known/security.txt` — <https://www.rfc-editor.org/rfc/rfc9116>
- OWASP ASVS — Application Security Verification Standard — <https://owasp.org/www-project-application-security-verification-standard/>
- AWS Budgets and Budget Actions — <https://docs.aws.amazon.com/cost-management/latest/userguide/budgets-managing-costs.html>
- AWS Cost Anomaly Detection — <https://docs.aws.amazon.com/cost-management/latest/userguide/getting-started-ad.html>
- AWS WAF rate-based rules — <https://docs.aws.amazon.com/waf/latest/developerguide/waf-rule-statement-type-rate-based.html>
- AWS S3 Object Lock — <https://docs.aws.amazon.com/AmazonS3/latest/userguide/object-lock.html>
- Canarytokens — <https://canarytokens.org/>
- `docs/operations/hardening.md` — the private (unadvertised) operations runbook holding the exploitable specifics: exact billing-alert thresholds, the full honeypot path list, circuit-breaker tuning, and lockout-schedule values (forthcoming; may be kept private to the hosted deployment).

## Notes

The single most important framing of this ADR is the public/private split. The project is public from commit one, and the default instinct is to document everything. For most of the architecture that instinct is correct and serves the AGPL transparency posture. For the deception layer and the precise tuning of the billing circuit breaker, full disclosure would degrade the defence — an attacker reading this file should learn that the defences exist and roughly how they are shaped, but not gain an operational map. `docs/operations/hardening.md` is where the map lives, and it is deliberately not advertised.

The billing circuit breaker is the one piece of this ADR that can, by design, take the public service offline without a human in the loop. That is a serious capability and is recorded as such. It is justified only because the failure it prevents — an unbounded invoice against a solo-maintained, fixed-budget project — is worse than a bounded, polite `503` that pages the maintainer. The breaker's refusal to auto-reset is deliberate: re-enabling the edge is a decision a human makes after understanding why the breaker tripped, in keeping with [CODE_OF_ETHICS.md instrument 48](../../CODE_OF_ETHICS.md) — fly the aircraft first, then diagnose, then re-open the doors.
