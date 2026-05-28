# ADR-002 — Release and deployment

| Field | Value |
| --- | --- |
| **Status** | Accepted |
| **Date** | 2026-05-28 |
| **Deciders** | @ICreateThunder |
| **Tags** | release, deployment, gitops, supply-chain, rollback |
| **Supersedes** | (none) |

## Context

Flight Academy is a multi-tenant aviation platform built by a single maintainer in the pre-1.0 phase. It must support two consumption modes from the same source tree: a hosted offering at `flight-academy.app` running on AWS K3s in `eu-west-2`, and operator-run self-host on anything from a single VM with `docker compose` through to a customer K8s cluster. Both modes are bound by AGPLv3 and the no-feature-gating commitment in [GOVERNANCE.md](../../GOVERNANCE.md#funding-and-commercial-relationships).

The forces at play:

- **Solo maintainer capacity.** Release plumbing must be largely unattended once tagged. Manual gates exist where regulators or users would expect them (security advisories, breaking changes), not as a substitute for automation.
- **Cost-constrained ARM nodes.** Production K3s runs on Graviton; control-plane footprint matters. A 300 MB controller is a meaningful fraction of node memory on a t4g.small.
- **AGPLv3 ethos.** The release pipeline must be transparent — public registry, public infra repo, signed artefacts that any third party can verify. Closed signing infrastructure is incompatible with the project's posture even where it is operationally simpler.
- **Regulated tenant base.** Flight schools, Part 145 maintenance organisations, and airfield operators are subject to UK CAA oversight (CAP 382 reporting, MOR under EU 376/2014 retained), GDPR, and in some cases EASA Part-ORA approvals. They will be audited. The release and deployment process must produce evidence that a given version came from a specific commit, was built without tampering, and was deployed at a known time. SBOM, signed images, and GitOps history exist to satisfy that audit, not to look good in marketing.
- **AWS infrastructure already in flight.** Cloudflare DNS, AWS ACM, S3+CloudFront for static assets, NLB+EC2 for the API tier, fck-nat for fixed egress IPs. The release process must terminate cleanly into this topology without requiring a re-platform.
- **Cluster template precedent.** Pre-flight experimentation and early research on the maintainer's other projects used Kustomize and ArgoCD. For Flight Academy we deliberately deviate from that template: the cost-constrained ARM nodes make Flux a better fit because of its smaller memory footprint, and `flux trace` provides field-grade SRE debugging (given any resource, it identifies the Kustomization that manages it — ArgoCD has no direct equivalent). Deviating is recorded as an explicit decision rather than a default; the reasoning is set out under decision D.
- **Restraint as a value.** [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instruments 35–36 ("Be not addicted to wine"; "Be not a great eater") read across to moderation as a project virtue. The release machinery should be as small as it can be while still doing its job. Every additional controller, registry, or service is a long-term cost.

What we do not yet know: whether self-hosters will overwhelmingly prefer Helm or `docker compose`; whether the ARM footprint advantage of Flux over ArgoCD persists once we add Flagger and the image-automation controllers; whether tenants will demand reproducible builds. These are tracked as open questions and may motivate later ADRs.

## Decision

We adopt a ten-part release and deployment design: two repositories (source and infra) with the source release pipeline driving the infra repo through GitHub Actions; container images published to `ghcr.io` with an ECR pull-through cache fronting AWS production; per-tag signed artefacts including multi-arch images, Helm chart, SBOMs, SLSA provenance, a self-host bundle, and an install script; Flux as the GitOps engine in place of ArgoCD specifically for this project; Flagger for progressive delivery on the Istio ambient mesh; database migrations as a dedicated K8s Job triggered by Flagger's pre-rollout webhook; the production topology described in section G; a single Rust binary that embeds the SvelteKit build for self-host via a Cargo feature; an install pattern that offers both a `curl | bash` convenience path and an equally documented reviewable path; and a strict rollback discipline that treats image rollback as routine and database schema rollback as forbidden.

The sub-decisions follow.

### A. Two repositories — source and infra

We split the public footprint into two repositories under the `flight-academy-uk` GitHub organisation:

| Repository | Contents | Licence | Visibility |
| --- | --- | --- | --- |
| `flight-academy-uk/flight-academy` | Rust API, SvelteKit web, Flutter mobile, Helm chart sources, `docker-compose.yaml` for self-host, all CI for the source tree, release workflow | AGPL-3.0-only | Public |
| `flight-academy-uk/flight-academy-infra` | Kustomize bases and overlays for the hosted environment, Flux configuration, SOPS-encrypted secrets, environment-specific values | AGPL-3.0-only | Public |

Releases are cut in the source repository. The release workflow, on a successful tag build, dispatches a workflow in the infra repository (via GitHub App token, OIDC-authenticated) that updates the image tag in the relevant overlay and opens a pull request against the infra repo's `main`. A maintainer merges (or auto-merge fires for patch releases with green checks); Flux reconciles; the cluster moves to the new version.

The infra repository is public for the same reason the source repository is: anyone — a tenant's CISO, a regulator, a curious contributor — should be able to inspect exactly what is deployed and how. Secrets committed to it are encrypted with SOPS and decrypted by Flux at reconcile time — using AWS KMS (accessed via IRSA) on the hosted deployment, or an `age` key on self-hosted installs; the public ciphertext is useless without the decryption key.

Two secret lifecycles are kept distinct: declarative, git-stored secrets (static configuration, issuer keys) live in the infra repo as SOPS-encrypted files, while rotated runtime credentials — chiefly per-Job database credentials — are never committed to git, sourced instead from AWS Secrets Manager via the External Secrets Operator and IRSA on the hosted deployment (SOPS on self-hosted installs).

Rejected: monorepo. Two CI domains intertwined — every infra change would re-run the source test matrix, every source change would re-run infra validation. The discipline of separating "what the software is" from "how this specific environment runs it" is worth the coordination cost.

Rejected: three repositories (source / chart / infra). The Helm chart lives in the source tree because chart values track the application's CLI flags and environment variables one-to-one. Splitting them creates a perpetual version-skew problem for no practical gain at this scale.

Rejected: Bitnami `sealed-secrets` for the git-stored secrets. It is Apache-2.0 and still maintained, but it is Bitnami/Broadcom-stewarded and on the same monetisation path that froze most of the free Bitnami catalogue in 2025 — not a dependency to anchor a new project on. Flux's native SOPS decryption also makes a separate sealed-secrets controller redundant, and SOPS is friendlier to self-hosters: an `age` key and no controller to run.

### B. Container registry — ghcr.io published, ECR pull-through cache for production

The source of truth for container images is GitHub Container Registry:

```text
ghcr.io/flight-academy-uk/flight-academy:vX.Y.Z
ghcr.io/flight-academy-uk/flight-academy:latest         (mutable, points to most recent stable)
ghcr.io/flight-academy-uk/charts/flight-academy:X.Y.Z   (OCI Helm chart)
```

These are public. Anyone — self-host operator, contributor, security researcher — can `docker pull` without authentication and verify the cosign signature without an account.

For the hosted production K3s cluster in `eu-west-2`, we configure an ECR pull-through cache that mirrors `ghcr.io/flight-academy-uk/*` on first pull and serves subsequent pulls from in-region ECR:

```text
<account>.dkr.ecr.eu-west-2.amazonaws.com/ghcr-cache/flight-academy-uk/flight-academy:vX.Y.Z
```

The rationale:

| Concern | Without pull-through cache | With pull-through cache |
| --- | --- | --- |
| Production depends on ghcr.io uptime | Yes — every node restart re-pulls | No — first pull caches, subsequent pulls are in-region |
| Cross-AZ egress charges | Hit on every node pull | Free; ECR to EC2 same-region is free |
| Public verification | Yes | Yes — ghcr.io is still the source of truth |
| Credentials in cluster | ghcr.io PAT (long-lived secret) | None — IRSA / Pod Identity issues short-lived ECR tokens |
| Storage cost | None to us | ~£0.08 / GB / month |

Authentication to ECR is via IAM Roles for Service Accounts (IRSA) or EKS Pod Identity equivalent on K3s, configured per the AWS documentation. There is no static ECR credential anywhere in the cluster — the kubelet's `ecr-credential-provider` exchanges the instance role for a registry token at pull time.

Rejected alternatives:

- **Docker Hub.** Rate-limited anonymous pulls (100/6h/IP) would break large fleets and CI. Paid plans solve the rate limit but not the central-point-of-failure concern, and add a third commercial registry to the picture.
- **Private ECR only.** Closes the supply chain to self-hosters. Incompatible with AGPLv3 ethos and with the published-Helm-chart workflow.
- **Quay.io.** No material advantage over ghcr.io; introduces a Red Hat / IBM dependency the project has no other reason to take on.
- **Self-hosted Harbor.** Operational burden on a solo maintainer for negligible benefit at this stage. Revisit if self-hosters demand an air-gap-friendly relay.

### C. Release artefacts per tag

A release is triggered by pushing a signed annotated git tag matching `vX.Y.Z` to the source repository. The release workflow runs on GitHub Actions hosted runners (no self-hosted runners in the release path — self-hosted runners are a known supply-chain risk and we accept the cost of GitHub's runners in exchange for keylessness via OIDC).

The following artefacts are produced and signed:

| Artefact | Format | Signing | Notes |
| --- | --- | --- | --- |
| API binary | `flight-academy-vX.Y.Z-{amd64,arm64}.tar.gz` | cosign keyless (Sigstore + GitHub OIDC) | Statically linked, hosted-mode build (no embedded static) |
| API binary, embedded | `flight-academy-embedded-vX.Y.Z-{amd64,arm64}.tar.gz` | cosign keyless | `--features embedded-static` — see section H |
| Container image | OCI, multi-arch (amd64 + arm64) | cosign keyless + SBOM attestation + SLSA provenance attestation | Tagged `vX.Y.Z`; `latest` updated only for stable releases (no pre-1.0 `latest` until 1.0 ships) |
| Helm chart | OCI chart at `ghcr.io/flight-academy-uk/charts/flight-academy:X.Y.Z` | helm-sign (PGP) **and** cosign keyless | Both are useful: helm-sign for `helm install --verify`; cosign for policy controllers |
| SBOM | CycloneDX 1.5 JSON + SPDX 2.3 JSON | cosign attestation against the image digest | Generated by `syft` |
| Provenance | SLSA v1.0 provenance | cosign attestation | Generated via `slsa-framework/slsa-github-generator`; meets SLSA Build Level 3 |
| Self-host bundle | `flight-academy-selfhost-vX.Y.Z.tar.gz` containing `docker-compose.yaml`, example `.env`, README, sample TLS config | cosign keyless | Operator drops it onto a host and runs `docker compose up` |
| Install script | `install.sh` | PGP-signed by maintainer hardware-backed key **and** cosign keyless | Hosted at `https://install.flight-academy.app`; see section I |

All artefacts are attached to a GitHub Release. The release is marked immutable (GitHub's release immutability setting is enabled at the repository level). Patch releases for security or correctness fixes are cut as `vX.Y.Z+1`; tags are never reused, never force-pushed, never deleted.

Cosign signatures are verifiable without an account:

```bash
cosign verify ghcr.io/flight-academy-uk/flight-academy:v0.1.0 \
  --certificate-identity-regexp 'https://github.com/flight-academy-uk/flight-academy/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
```

The transparency log entry is in Sigstore's Rekor instance and is queryable indefinitely.

### D. GitOps engine — Flux, deviating from the cluster template's ArgoCD

For Flight Academy specifically we use **Flux v2**, not ArgoCD. This is a deliberate deviation from the maintainer's [cluster-project-template](../../GOVERNANCE.md) default and is documented as such so future contributors do not interpret it as inconsistency.

Reasons specific to Flight Academy:

- **Memory footprint.** A typical ArgoCD installation runs around 450–550 MB across the API server, repo server, application controller, and Dex. Flux's full controller set (source, kustomize, helm, notification, image-reflector, image-automation) runs around 180–230 MB. On a small t4g.small fleet that difference is roughly 15–20% of each worker node's 2 GB — meaningful headroom that goes to the application instead of the GitOps controller.
- **Native image automation.** Flux's `image-reflector-controller` + `image-automation-controller` is in-tree. ArgoCD's equivalent (Argo Image Updater) is a separate, community-maintained add-on with a smaller blast radius of testing. Image automation is on the critical path of our release flow — having it in the same project as the GitOps engine matters.
- **Less surface area.** Flux has no web UI by default. The cluster does not need to expose a GitOps dashboard; `flux` CLI plus Grafana for status is sufficient and reduces the attack surface.

Costs of this deviation:

- The cluster-wide bootstrap (cert-manager, monitoring) elsewhere uses ArgoCD's app-of-apps. We will accept running both controllers on the same cluster if integration with shared cluster-wide tooling requires it. Both can co-exist; they reconcile different `kind:` resources.
- Contributors familiar with ArgoCD's UI will need to learn `flux` CLI workflows.

The mental model — useful for anyone arriving at the codebase later:

```text
GitRepository                  (Flux source: "watch this git repo at this ref")
        |
        v
Kustomization / HelmRelease    (Flux consumer: "apply manifests from that source")
        |
        v
Deployment / StatefulSet ...   (the actual workloads)

ImageRepository                (Flux: "scan this OCI repo for tags")
        |
        v
ImagePolicy                    (Flux: "tags matching semver >=0.1.0 <0.2.0")
        |
        v
ImageUpdateAutomation          (Flux: "commit the new tag to this git path")
```

Image-automation commits land in `flight-academy-infra` and are co-signed by the controller's GitHub App identity, signed-off with `Signed-off-by: flux-image-automation`, and verifiable in the infra repo's history.

An illustrative `GitRepository` and `Kustomization` pair in the infra repo:

```yaml
apiVersion: source.toolkit.fluxcd.io/v1
kind: GitRepository
metadata:
  name: flight-academy-infra
  namespace: flux-system
spec:
  interval: 1m
  url: https://github.com/flight-academy-uk/flight-academy-infra
  ref:
    branch: main
  secretRef:
    name: github-deploy-key
---
apiVersion: kustomize.toolkit.fluxcd.io/v1
kind: Kustomization
metadata:
  name: flight-academy-hosted-prod
  namespace: flux-system
spec:
  interval: 5m
  path: ./overlays/hosted-prod
  prune: true
  sourceRef:
    kind: GitRepository
    name: flight-academy-infra
  timeout: 5m
```

### E. Progressive delivery — Flagger canary on Istio ambient

Flagger runs in the cluster alongside Flux and drives canary releases on the Istio ambient mesh. A typical `Canary` resource for the API:

```yaml
apiVersion: flagger.app/v1beta1
kind: Canary
metadata:
  name: flight-academy-api
  namespace: flight-academy
spec:
  targetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: flight-academy-api
  progressDeadlineSeconds: 600
  service:
    port: 8080
    targetPort: 8080
  analysis:
    interval: 1m
    threshold: 5
    maxWeight: 100
    stepWeight: 10
    metrics:
      - name: request-success-rate
        thresholdRange:
          min: 99
        interval: 1m
      - name: request-duration-p99
        thresholdRange:
          max: 500
        interval: 1m
    webhooks:
      - name: db-migrate
        type: pre-rollout
        url: http://migration-runner.flight-academy/run
        timeout: 5m
      - name: smoke-test
        type: post-rollout
        url: http://smoke-runner.flight-academy/run
        timeout: 2m
```

Canary criteria (these are the production values; staging is more permissive to give faster feedback to PR previews):

| Criterion | Threshold | Source |
| --- | --- | --- |
| Step weight | 10 % per step | Flagger |
| Step interval | 1 minute | Flagger |
| Total time to 100 % | ~10 minutes | Derived |
| Request success rate | > 99 % | Prometheus, from Istio ambient telemetry |
| p99 request latency | < 500 ms | Prometheus |
| Consecutive failures before rollback | 5 | Flagger `threshold: 5` |
| Pre-rollout hook | Migration Job (see F) | Flagger webhook |
| Post-rollout hook | Smoke-test runner | Flagger webhook |

Notifications go through Flux's `notification-controller`, posting to Slack on canary start, promotion, and rollback. There is no email notification channel — operator email goes via Cloudflare Email Routing for inbound only; we do not send outbound transactional email from CI.

The 500 ms p99 threshold reflects expected end-user budgets for the booking and dispatch flows; we will tighten it as we collect production baselines. The 99 % success rate excludes 4xx responses (those are user-induced and a deployment cannot fix them); only 5xx counts as failure for the canary.

### F. Database migrations — dedicated Job, not application startup

Database migrations run in a dedicated Kubernetes Job, **not** during application pod startup. The Job is triggered by Flagger's pre-rollout webhook (see section E) before any traffic shifts onto the canary pods.

Role separation at the database level:

| Role | Permissions | Used by |
| --- | --- | --- |
| `app_migrator` | DDL on the application schema, DML during migrations | The migration Job only |
| `app_api` | DML on the application schema; **no DDL** | The running API pods |
| `app_read_only` | SELECT on the application schema | Read-replica consumers, analytics jobs |

The application pod cannot execute DDL even if compromised. Migration credentials are issued only to the Job's service account — from AWS Secrets Manager via the External Secrets Operator and IRSA, or from SOPS on self-hosted installs — and are not present in the API pod's environment.

Forward-only is non-negotiable. Reverse migrations (`down` files) are not generated and not run. Any schema change that would be destructive is split into an expand step (new column / table / index added, dual-write or default-aware code) and a contract step (old column dropped) deployed in separate releases, with the expand step always landing first and being safe to roll back at the code level.

The deeper migration discipline — naming, ordering, checksum validation, `pg_dump` of the schema in CI as an integrity check — lives in [ADR-003 — Database migration discipline](ADR-003-db-migrations.md). This ADR records only the deployment-time mechanics.

### G. Hosted production topology

The hosted offering at `flight-academy.app` runs in AWS `eu-west-2` (London). Topology, end to end:

```text
End user
  |
  v
Cloudflare DNS (DNS-only, not proxied)
  |
  v
AWS CloudFront (separate distributions for app + API)
  | -- app: serves SvelteKit static build from S3 origin
  | -- api: forwards to NLB; AWS WAF managed rules in front
  v
AWS Network Load Balancer
  |
  v
Istio ingress gateway (ambient mode) on K3s
  |
  v
Flight Academy API pods + workers
  |
  v
PostgreSQL (CloudNativePG) | MinIO | (other services per ADR-001)

Outbound (webhooks, third-party API calls):
  pods -> egress gateway -> fck-nat per AZ -> three fixed Elastic IPs
```

Components:

| Layer | Service | Notes |
| --- | --- | --- |
| Authoritative DNS | Cloudflare | DNS-only mode. We do **not** use Cloudflare's proxy for the API path; that would interpose a third party on encrypted traffic and is incompatible with the no-telemetry posture. App static traffic is fronted by CloudFront. |
| Inbound email | Cloudflare Email Routing | Free aliasing for `security@`, `abuse@`, `noreply@`, etc., forwarded to maintainer mailboxes. We do not run our own MX. |
| Static frontend | S3 + CloudFront | SvelteKit `adapter-static` build. ACM cert in `us-east-1` (CloudFront's requirement). |
| API edge | CloudFront + AWS WAF | WAF managed rule groups: Core Rule Set, Known Bad Inputs, IP Reputation, Anonymous IPs. Custom rate-limit rules per ADR-004. |
| API load balancer | NLB (TCP / TLS passthrough) | ACM cert in `eu-west-2`. Health checks against the Istio ingress gateway readiness probe. |
| Mesh | Istio ambient | Layer-4 ztunnel; layer-7 waypoint proxies per namespace where needed. |
| Cluster | K3s on EC2 (Graviton) | Three nodes minimum across three AZs. Etcd embedded. |
| Database | CloudNativePG | Three-replica cluster, synchronous replication across AZs. WAL archive to S3 with object-lock retention. |
| Object store | MinIO | Erasure-coded across nodes; for hosted, S3 may eventually replace MinIO for tenant uploads — under review. |
| Egress | fck-nat per AZ | Three fixed Elastic IPs (one per AZ). Published in operator docs so tenant firewalls can allow-list our outbound webhooks. |

We use two ACM certs (us-east-1 for CloudFront, eu-west-2 for NLB) rather than a single wildcard, because wildcard certs increase blast radius if compromised. Both certs are issued for `flight-academy.app` and `*.flight-academy.app`.

The fixed egress IPs matter for the regulated tenant base: customers' IT teams will often need to allow-list our IPs in their firewalls for inbound webhook receipt, and we owe them stable, documented endpoints.

### H. Self-host topology — single binary, embedded static

For self-host, we publish a Rust binary that serves both the API and the SvelteKit static frontend from the same process and port. Implementation: a Cargo feature `embedded-static` enables the [`rust-embed`](https://crates.io/crates/rust-embed) crate, which compiles the SvelteKit build output into the binary at build time. The embedded build serves `index.html` and the asset directory directly from memory; the API handlers are unaffected.

```toml
[features]
default = []
embedded-static = ["rust-embed", "mime_guess"]
```

The hosted build does **not** enable the feature. CloudFront serves the static frontend from S3; the API binary serves only API routes. This keeps the hosted cache strategy (CloudFront edge caching of static assets) separate from API behaviour, and keeps the hosted binary smaller.

Two binary artefacts are published per release accordingly:

| Variant | Cargo features | Use |
| --- | --- | --- |
| `flight-academy` | default | Hosted production; expects an external static server |
| `flight-academy-embedded` | `embedded-static` | Self-host; serves both API and frontend from one process |

The self-host container image is the embedded variant. The `docker-compose.yaml` bundled in the self-host artefact (section C) brings up Postgres, MinIO, and the embedded API on a single host:

```yaml
services:
  flight-academy:
    image: ghcr.io/flight-academy-uk/flight-academy-embedded:v0.1.0
    ports:
      - "8080:8080"
    depends_on:
      - postgres
      - minio
    environment:
      DATABASE_URL: postgres://app_api:...@postgres/flight_academy
      OBJECT_STORE_URL: http://minio:9000
      # ...
```

A self-host operator who wants to terminate TLS in front uses Caddy, Traefik, or nginx as a reverse proxy. We do not bundle a reverse proxy in the default compose stack — that decision belongs to the operator.

The size cost of the embedded variant is acceptable: the SvelteKit production build for the initial scope is on the order of 2–3 MB gzipped, which adds <10 MB to the final binary.

### I. Install pattern — `curl | bash` with an equally-documented reviewable path

For self-host operators we publish an install script reachable at:

```text
https://install.flight-academy.app
```

This is the canonical URL. `https://flight-academy.app/install` is a 301 to the canonical subdomain so we can serve the install endpoint from a different origin (S3 static + CloudFront) independent of the main app's deployment cadence.

The convenience invocation:

```bash
curl -fsSL https://install.flight-academy.app | bash
```

The reviewable invocation, documented with equal prominence in the README and the self-host guide:

```bash
curl -fsSL https://install.flight-academy.app -o install.sh
curl -fsSL https://install.flight-academy.app/install.sh.sig -o install.sh.sig
gpg --auto-key-locate wkd --locate-key security@flight-academy.app
gpg --verify install.sh.sig install.sh
less install.sh
bash install.sh
```

The maintainer's PGP key for signing the install script is the same key documented in [SECURITY.md](../../SECURITY.md) (hardware-token-backed, Ed25519). The cosign signature is also published and can be verified without a PGP toolchain:

```bash
cosign verify-blob \
  --signature install.sh.sig.cosign \
  --certificate install.sh.crt \
  --certificate-identity-regexp 'https://github.com/flight-academy-uk/flight-academy/.*' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  install.sh
```

What the script does:

1. Detect host OS (Linux only at launch; macOS support tracked separately).
2. Detect container runtime (`docker` or `podman`). Refuse to proceed if neither is present.
3. Download the self-host bundle tarball for the requested version (latest stable by default; pinnable via `FA_VERSION=v0.1.0`).
4. Verify the bundle's cosign signature. Refuse to proceed on signature failure with no `--insecure` escape hatch — if you do not trust the signature, you should not run the bundle.
5. Extract the bundle to `/opt/flight-academy` by default (overridable via `FA_PREFIX`).
6. Generate strong random initial passwords for Postgres and MinIO, write them to `/opt/flight-academy/.env` with `0600` permissions.
7. `docker compose up -d`.
8. Wait for the readiness endpoint; print the completion URL and the path to the credentials file.

The script is idempotent on re-run: re-running upgrades the bundle in place, preserves credentials, runs the migration Job, and restarts containers. There is no separate `upgrade` command.

The `curl | bash` form is widely (and reasonably) criticised. The criticism is largely about the *absence of an alternative*, not the form itself. Our response is to document both paths with equal prominence, to publish signed binaries, and to ensure the script is short enough to actually read (target: under 400 lines including comments). Operators in this audience are sysadmins, not consumer end users — they can be expected to verify a signature, and we owe them the ability to do so easily.

Rejected: package-manager-only distribution (apt / yum / brew). These are good additions but slow to update and vary by distribution. We will publish to package managers once the project stabilises; until then the install script is the canonical entry point.

Rejected: a GUI installer. Over-engineering for the operator audience and out of step with how Linux services are typically installed in 2026.

### J. Rollback discipline

The asymmetry between image rollback and database rollback is the most important operational principle here. Treating them as symmetric is the single most common cause of self-inflicted production outages in the industry; we will not.

**Image rollback — routine.** Reverting the image tag in `flight-academy-infra` is a normal git revert. Flux reconciles, Flagger canaries the previous version back in, traffic shifts. The full mechanic:

```bash
cd flight-academy-infra
git revert <sha-of-tag-bump-commit>
git push origin main
# Flux picks up within the reconcile interval; Flagger canaries the rollback.
```

This is the same code path as a forward release, with the previous tag as the target. There is no special "rollback" mode.

**Database schema rollback — forbidden.** We do not run reverse migrations. The risk is data loss: any reverse migration that drops a column or table also drops every row of data added since the forward migration ran. There is no general way to reconcile that data back into the prior schema. The cure is worse than the disease.

The discipline that makes image rollback safe in the presence of schema changes is the **parallel-change rule**: code at version N-1 must continue to work against schema N. This is enforced by:

- Schema changes are always expand-then-contract over at least two releases.
- The expand release adds the new column / table / index but does not require it. Code at version N continues to write the old shape and read both.
- The contract release (next minor or later) removes the old shape only after operators have had time to upgrade.
- CI runs an integration test matrix that pairs application version N-1 against schema N, blocking any release that breaks this.

The details — the matrix construction, the schema diff checks, how this interacts with the migration Job — live in [ADR-003 — Database migration discipline](ADR-003-db-migrations.md).

**Automatic rollback during canary.** Flagger rolls back automatically when canary metrics breach (section E). The maintainer is notified via Slack but no human action is required; the deployment is reverted to the previously-stable revision before damage spreads. This is the normal failure mode for a bad release and is by design.

**Manual rollback runbook.** A step-by-step runbook for the rarer case (rollback needed outside a canary window — e.g., a bug surfaces hours after promotion) will live at `docs/operations/rollback-runbook.md`. It is currently a placeholder; the placeholder is acceptable for pre-1.0 because we will not have run production for long enough to need it. Before 1.0 it will be filled in and exercised in a game-day drill.

## Consequences

### Positive

- **Audit-grade supply chain.** Every release produces a signed image, an SBOM, SLSA Level 3 provenance, a signed Helm chart, and a signed self-host bundle. Any tenant or regulator can verify what is running from public information alone.
- **Self-hosters are first-class.** The same image, the same chart, the same install script we use ourselves. No hidden gates, no enterprise edition. This satisfies the [GOVERNANCE.md](../../GOVERNANCE.md#funding-and-commercial-relationships) commitment.
- **Production decoupled from ghcr.io.** ECR pull-through cache means a GitHub outage does not take production down; only new tag pulls would block, and a running deployment continues to serve.
- **Memory savings on production nodes.** Flux's lighter controller footprint frees ~250 MB per cluster for application workloads — meaningful on the cost-controlled ARM fleet.
- **Image rollback is trivially safe.** Git revert in the infra repo, Flux reconciles. No special mode, no risk-of-data-loss path.
- **Single-binary self-host is the simplest possible operator experience.** One process, one port, one set of logs. The default `docker compose` stack has three services (binary, Postgres, MinIO) which is close to the floor for a useful deployment.
- **Canary catches bad releases automatically.** A regression in p99 latency or error rate triggers rollback before damage spreads.
- **Cross-reference of restraint** with [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) instruments 35–36 (moderation, read across to dependencies and tooling): we chose Flux over ArgoCD partly to reduce the controller footprint, and we are deliberately not running a self-hosted registry, a self-hosted email server, or a homegrown GitOps engine.

### Negative

- **Two repositories require coordination.** A change that touches both source CI and infra config must be split across two PRs. The release automation hides most of this, but the maintainer cost of two repositories instead of one is real.
- **Deviation from cluster-template ArgoCD.** Future contributors who have worked with the maintainer's other projects will need to learn Flux for this one. Documented above to limit the surprise; cost is real.
- **Flagger requires Istio metrics to be working.** A Prometheus outage during deploy would block canary progression. We accept this — a metric pipeline that is not working is itself a reason not to be deploying.
- **`rust-embed` increases build time.** Embedding the SvelteKit build into the Rust binary adds a compile step that re-runs whenever the frontend changes. CI caching mitigates but does not eliminate the cost.
- **`curl | bash` is a real, defensible criticism vector.** We have the reviewable path and the signatures, but some operators will still object on principle. We accept this and direct them to the reviewable invocation.
- **ECR pull-through cache storage is a small ongoing AWS bill.** Estimated under £5 / month at current image sizes; budgeted.
- **No reverse migrations means more discipline at design time.** Every schema change is a two-release dance. This is a feature, not a bug — but it costs developer time.
- **PGP key custody is a single-point-of-failure on the maintainer's hardware token.** Documented mitigation: revocation certificate held offline; second hardware token as backup; transition plan documented in SECURITY.md once maintainer count grows.

### Neutral

- **Cosign keyless ties signing identity to GitHub OIDC.** Migrating away from GitHub would require re-issuing signatures with a different identity provider. Acceptable for now; tracked.
- **fck-nat per AZ is cheap but is one more thing to monitor.** Three NAT instances vs one AWS managed NAT Gateway. Cost savings substantial; alarms in place.
- **The infra repo being public means cluster topology is publicly known.** This is consistent with the project's transparency posture and the secrets-are-encrypted model, but operators with a private-infra preference should fork the infra repo and run their own.
- **Helm chart distribution as OCI (not a chart museum / Artifact Hub indexer).** Modern Helm supports OCI directly; we accept that some chart-museum tooling will not discover us via traditional indexes.

## Alternatives considered

### Alternative — monorepo (source + infra in one repository)

A single repository would simplify cross-cutting changes and remove the need for an inter-repo workflow dispatch. Rejected because every infra change would trigger the source CI matrix (currently estimated ~12 minutes for a full Rust + SvelteKit + Flutter run) and vice versa, wasting CI minutes and slowing feedback. The separation of "what the software is" from "how this specific environment runs it" is also conceptually clearer. Would reconsider if maintainer count grew enough that the inter-repo PR overhead became material, or if GitHub Actions added cheap path-based job skipping that genuinely respected the boundary.

### Alternative — ArgoCD instead of Flux

ArgoCD has a better UI, broader community familiarity, and is the default in the maintainer's other projects. Rejected for Flight Academy specifically because of the controller memory footprint and because image-automation is in-tree in Flux rather than a separate add-on. The deviation is documented (see section D). Would reconsider if the maintainer team grows and contributors arriving with ArgoCD experience would benefit more from familiarity than the project benefits from the memory delta.

### Alternative — Docker Hub as primary registry

Lower-friction for first-time pullers; ubiquitous in tutorials. Rejected primarily because of anonymous-pull rate limits, which would punish CI fleets and large self-host deployments. Paid Docker plans solve the rate limit but introduce a commercial registry contract that the project does not otherwise need. Would reconsider if Docker changed its rate-limit policy meaningfully and offered an account-less verifiable signing story that competes with cosign keyless.

### Alternative — Private ECR only, no public registry

Closes the supply chain and breaks self-host. Rejected as incompatible with AGPLv3 ethos and with the no-feature-gating commitment.

### Alternative — Self-hosted Harbor

A self-hosted registry would give us full control of the image distribution. Rejected because operating a registry is a non-trivial commitment for a solo maintainer — backups, HA, signing key management, garbage collection — and the public-registry use case (anyone can pull and verify) is poorly served by self-hosted Harbor without an aggressive caching CDN in front of it. Would reconsider only if we encountered specific tenant requirements (air-gap relay, region-of-residency requirements) that ghcr.io + ECR cannot satisfy.

### Alternative — on-host self-host (no containers)

A static binary plus systemd unit, with Postgres installed via the host's package manager. Some operators prefer this; it has fewer moving parts than `docker compose`. Rejected as the *default* path because it makes the dependency on Postgres version, MinIO version, and TLS provisioning into a per-distro support matrix the maintainer cannot sustain. The static binaries are published anyway (section C), so an operator who wants the on-host path can take it; we just do not promise to support every distribution's particulars. Would reconsider promoting it to first-class if a contributor steps up to own a packaging matrix.

### Alternative — separate hosted and self-host binaries with no embedded-static feature

The hosted build and the self-host build could be entirely different binaries with different command surfaces. Rejected because the resulting code drift — hosted-only handlers, self-host-only handlers, divergent configuration — has been the source of bugs in every project the maintainer has seen attempt it. The Cargo-feature toggle keeps the divergence to a single linker-time decision: the embedded variant additionally compiles in `rust-embed` and a static-asset handler. All API code is identical between variants. CI builds both per release.

### Alternative — manual deployments (no GitOps)

`kubectl apply` from a maintainer's laptop. Rejected because it is unauditable: there is no canonical record in git of what was applied when, and a maintainer-laptop compromise becomes a cluster compromise. GitOps with signed commits in the infra repo is the auditable equivalent.

### Alternative — Argo Rollouts instead of Flagger

Argo Rollouts has analogous canary capability. Chose Flagger because it integrates cleanly with Istio ambient (where Argo Rollouts integration is newer and less battle-tested) and because we already chose Flux as the GitOps engine — staying in the Flux/Flagger ecosystem reduces the project's controller surface. Would reconsider if the Flagger maintenance velocity slowed materially or if a specific feature in Argo Rollouts (e.g., experiment CRDs) became necessary.

### Alternative — running migrations as an `initContainer` on the API pods

Simpler to wire up than a dedicated Job. Rejected because every API pod would attempt the migration on startup, requiring application-level locks; because the API pod's service account would need DDL permissions it should not have; and because a migration failure would crash-loop application pods instead of surfacing as a single Job failure with clear logs. The dedicated Job is the discipline that lets us keep `app_api` strictly DML-only.

## References

- [ADR-001 — Platform architecture](ADR-001-platform.md) — informs the API surface that gets released and the components in the deployment topology.
- [ADR-003 — Database migration discipline](ADR-003-db-migrations.md) — referenced from section F and section J for the forward-only and parallel-change rules.
- [ADR-004 — Defence in depth](ADR-004-defence-in-depth.md) — informs the WAF and rate-limit rules referenced in section G.
- [CONTRIBUTING.md](../../CONTRIBUTING.md) — DCO, signed commits, conventional commits. The release workflow assumes these are upheld in `main`.
- [GOVERNANCE.md](../../GOVERNANCE.md) — release-and-signing-process changes are an ADR-class decision per the governance criteria.
- [SECURITY.md](../../SECURITY.md) — supply-chain threat model; PGP key for the install script; supported-versions policy that determines which tags receive security backports.
- [CODE_OF_ETHICS.md](../../CODE_OF_ETHICS.md) — instruments 35–36 (restraint, no superfluous tooling); instrument 48 (watchfulness, applied here to release plumbing).

External:

- Sigstore — <https://docs.sigstore.dev/>
- SLSA v1.0 specification — <https://slsa.dev/spec/v1.0/>
- Flux — <https://fluxcd.io/flux/>
- Flagger — <https://docs.flagger.app/>
- CloudNativePG — <https://cloudnative-pg.io/>
- AWS ECR pull-through cache — <https://docs.aws.amazon.com/AmazonECR/latest/userguide/pull-through-cache.html>
- Cloudflare Email Routing — <https://developers.cloudflare.com/email-routing/>
- `rust-embed` — <https://crates.io/crates/rust-embed>
- Developer Certificate of Origin — <https://developercertificate.org/>

## Notes

The choice of Flux over ArgoCD for this project specifically is the deviation most likely to draw questions from contributors familiar with the maintainer's other work. It is recorded here explicitly so that there is a single document to point to when the question arises, and so the decision can be re-evaluated against current numbers when conditions change — particularly if Flagger gains better ArgoCD integration, or if ArgoCD's controller footprint drops materially, or if the cluster's free-memory budget changes.

The `docs/operations/rollback-runbook.md` referenced in section J is a deliberate placeholder. Writing a runbook ahead of production experience produces theatre, not operations. Before 1.0 it will be filled in based on at least one game-day drill and any real incidents we have accumulated by then.
