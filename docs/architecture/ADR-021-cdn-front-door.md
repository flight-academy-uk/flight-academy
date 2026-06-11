# ADR-021 — CDN front-door — Cloudflare; origin runtime contract over vendor

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-06-11 |
| **Deciders** | @ICreateThunder |
| **Tags** | infra, cdn, edge, runtime, interfaces, security |
| **Supersedes** | (none — refines [ADR-001 §A](ADR-001-platform.md), [ADR-019 §B](ADR-019-white-label-runtime.md), [ADR-020 §H](ADR-020-mash-frontend-architecture.md)) |

## Context

[ADR-001 §A](ADR-001-platform.md), [ADR-019 §B](ADR-019-white-label-runtime.md), and [ADR-020 §H](ADR-020-mash-frontend-architecture.md) name CloudFront for the hosted-data-path CDN and name vendor-specific origin primitives (a particular CDN, a particular load balancer, a particular object-store SKU, a particular compute substrate). Two refinements emerge as the cluster onboarding work firms up:

1. **Edge: Cloudflare fits FA's posture better than CloudFront.** Service quality is the load-bearing concern — Cloudflare's L3/L4 DDoS absorption is materially better than typical cloud-provider default-tier DDoS, and the paid-tier capability ceiling (Super Bot Fight Mode, expanded rate-limit rule count, Cache-Tag granularity, managed challenge tuning) is higher in shape than cloud-provider WAF stacks. Cost structure is also more predictable than CloudFront's variable-egress-plus-per-rule-WAF model. Integration symmetry with Cloudflare DNS removes multi-vendor edge coordination.

2. **Origin: the runtime interfaces are what's architecturally load-bearing, not the cloud vendor.** FA's substrate is defined by a small set of named interfaces — already specified across ADR-001 §A and ADR-004 — and any provider satisfying them is interchangeable from the application's perspective. Specific provider selection (which cloud, which dedicated host, which region) is operational, not architectural, and lives in infra-repo runbooks.

[ADR-019 §B](ADR-019-white-label-runtime.md) requires the edge to forward 103 Early Hints unchanged for the brand-asset preload mechanism — Cloudflare does; CloudFront's support is partial. Aviation-safety surfaces (magic-link, MOR submission) need real, not nominal, DDoS protection at the floor.

Forces: protect aviation-safety surfaces with real not nominal DDoS protection ([CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instrument 28); restraint on multi-vendor edge configurations (35–36); decouple architecture from vendor lock-in by naming interfaces rather than products (24 — no pretence; the interfaces are stable, the provider question genuinely reduces to operational fit).

## Decision

**Two coupled decisions:**

1. **Cloudflare provides DNS + CDN + WAF + L3/L4 DDoS at the edge.** The marketing surface, the application HTML / JSON path, and any per-tenant subdomain traffic share this single Cloudflare edge. Cloud-provider WAF and default-tier DDoS protections are not relied upon; they are the floor that Cloudflare meaningfully exceeds.

2. **The origin runtime is defined by interfaces, not vendor.** The hosted cluster MUST satisfy:
   - **k3s** as the Kubernetes distribution
   - **Cilium** as the CNI with host networking + NetworkPolicy default-deny
   - **Istio ambient** for L4/L7 mesh (z-tunnels; no sidecars)
   - **CNPG** as the Postgres operator
   - **An S3-compatible object store** for cluster-side static assets and audit cold archive (MinIO inside the cluster is the canonical implementation; provider-managed equivalents are permitted)
   - **cert-manager + Let's Encrypt** (DNS-01 via Cloudflare API) for wildcard origin TLS bound to the Istio Gateway

ADR-001 §A's vendor-specific origin references — the particular CDN, the particular load balancer SKU, the particular object-store name, the particular compute substrate — read through this ADR as **instances** of the broader runtime contract. Specific provider selection (cloud, dedicated host, region, ingress LB) lives in infra-repo runbooks; this ADR's edge decision composes with any origin satisfying the named interfaces.

Implementation details that follow — wildcard cert issuance, edge security-group restriction to Cloudflare's published IP ranges, Cloudflare API token storage, Workers vs origin-served edge logic — are infrastructure runbook items, not load-bearing here.

## Consequences

### Positive

- **DDoS protection is real, not nominal.** Cloudflare's free-tier L3/L4 absorption genuinely exceeds typical cloud-provider default-tier DDoS.
- **Capability ceiling is higher under one vendor.** Super Bot Fight Mode, Cache-Tag purge, expanded rate-limit rules, managed challenge — capabilities cloud-provider WAF + default-DDoS don't match in shape.
- **One edge surface to operate.** Cache policy, rate-limit rules, WAF tuning, 103 forwarding all live in one console; one set of credentials, one runbook.
- **103 Early Hints works.** [ADR-019 §B](ADR-019-white-label-runtime.md) brand-asset preload is supported without caveat.
- **Cost structure is predictable.** Free tier covers typical v0.1 traffic; cost concerns do not gate decisions.
- **Origin is vendor-portable.** The cluster can move between cloud providers, between cloud and dedicated host, or between regions without application-code changes. The interface contract is what FA writes against; the vendor is what the infra repo selects.
- **Marketing pages, app pages, and API endpoints share the same edge** without coordination across providers.

### Negative

- **Vendor surface is Cloudflare** at the edge. Moving off later requires DNS migration + cache repopulation + WAF re-authoring. Bounded to infra.
- **Cloud-provider-specific WAF rulesets unavailable.** Cloudflare's catalogue is functionally adequate at the paid tier but differently shaped (no cloud-provider Bot Control add-on, no provider-managed rules library).
- **Free-tier ceiling.** Rate-limit rule count, per-tenant Cache-Tag purge, bot-management — practically a v1 budget item, known.
- **No Lambda@Edge or equivalent.** Edge personalisation, if it materialises as a need, takes the Cloudflare Workers path — deferred until concrete pressure exists.
- **Interface contract is not unilateral.** Replacing a named interface (e.g. swapping CNPG for a different Postgres operator, Istio ambient for a sidecar-based mesh) would require an ADR amending this one. The contract is stable but not arbitrary.

### Neutral

- **Object-store interface stays S3-compatible.** MinIO inside the cluster or provider-managed equivalent — same application code path either way.
- **Cluster ingress L4 LB is provider-specific.** Some providers offer one as a managed primitive; some require operator-managed equivalents. Either satisfies the contract.
- **Origin cert lifecycle moves from cloud-provider-managed certificate services to cert-manager.** Operationally similar; trades cloud-provider IAM for Cloudflare API token surface.

## Alternatives considered

### Stay with CloudFront + cloud-provider WAF + cloud-provider default-tier DDoS

Rejected on service quality (Cloudflare's free DDoS materially exceeds typical default-tier) and cost structure (per-rule WAF + variable egress accumulates unpredictably). Defensible in a fully cloud-native organisation already paying for premium DDoS, managed Kubernetes, and managed certificates; FA's economics and operational posture do not favour this.

### Fastly

Edge CDN comparable in capability; rejected because Fastly is paid from day one and offers no capability advantage over Cloudflare's paid tier at FA's scale.

### Pin origin to a specific provider in the ADR

Rejected — vendor choice for cost or feature reasons is genuinely operational, not architectural. Naming the provider in an immutable ADR creates friction every time the cost / capability / region fit shifts (which it does, early in a project). Naming interfaces lets the architecture stay stable while the infra repo handles provider selection.

### No CDN, cluster ingress LB directly user-facing

Rejected — DDoS protection alone justifies an edge.

## References

- Refines [ADR-001 §A](ADR-001-platform.md), [ADR-019 §B](ADR-019-white-label-runtime.md), [ADR-020 §H](ADR-020-mash-frontend-architecture.md) — CDN layer + vendor-specific origin references reinterpreted as interface instances.
- Composes with [ADR-002 §D/§E](ADR-002-release-deployment.md) — Flux + Flagger + SOPS pattern is interface-stable across providers.
- Composes with [ADR-004 §A/§B](ADR-004-defence-in-depth.md) — circuit breaker + rate-limiting layered at Cloudflare edge with in-cluster `tower_governor` as defence in depth.
- Cloudflare 103 Early Hints support — <https://developers.cloudflare.com/cache/advanced-configuration/early-hints/>.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 24 (no pretence — vendor abstraction is genuine because the interfaces are stable; not theatrical), 28 (truth — Cloudflare Free's DDoS is real where default-tier is nominal), 35–36 (restraint — name interfaces, not products).

## Notes

The interface contract is the durable artefact of this ADR. Current candidate providers are tracked in infra-repo runbooks; ADR amendments are not required when the candidate shifts. An ADR amendment IS required when the contract itself shifts — replacing CNPG, replacing Cilium, replacing Istio ambient, or removing the S3-compatible interface would all warrant a successor ADR. The contract is stable but not arbitrary.
