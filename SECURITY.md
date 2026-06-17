# Security policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.x (pre-1.0) | latest minor only |
| 1.x | latest minor + previous minor (once 1.0 ships) |

Pre-1.0 releases receive security fixes on the latest minor branch only. Once 1.0 ships, the policy expands to the two most recent minor branches.

## Reporting a vulnerability

**Do not** open public issues for security vulnerabilities.

**Preferred:** [GitHub Private Security Advisories](https://github.com/flight-academy-uk/flight-academy/security/advisories/new). Encrypted in transit, scoped to maintainers, audit-logged.

**Fallback:** email `security@flight-academy.app` (mailbox to be live before any public release; PGP fingerprint published below).

### What to include

- Affected version (release tag or commit SHA if running from source)
- Environment (hosted instance vs self-host)
- Reproducer or proof of concept
- CVSS 3.1 vector if you have assessed it
- Whether you want public credit

### Response SLAs

| Stage | Target |
|---|---|
| Acknowledgement of receipt | 48 hours |
| Initial assessment | 5 working days |
| Coordinated disclosure (typical) | 90 days from initial assessment |

Maintainers will provide status updates at least every 7 days during active investigation. If you do not hear back within the acknowledgement window, please follow up — the message may not have reached the maintainers.

## PGP key

A PGP key is available for signed and encrypted security correspondence.

| | |
|---|---|
| **Primary UID** | Robert Shalders &lt;robert@shalders.co.uk&gt; |
| **Project UID** | Flight Academy Security &lt;security@flight-academy.app&gt; |
| **Fingerprint** | `1A44 8CE4 18BD 8D37 1D12  B697 418D 45B7 1F57 D61F` |
| **Algorithms** | Ed25519 (sign) / Curve25519 (encrypt) |
| **Hardware** | Hardware-token-backed; private key material never leaves the device |

Fetch the public key from any of these sources:

- **Keyserver** (verified): <https://keys.openpgp.org/search?q=security@flight-academy.app>
- **WKD** (auto-discovery): `gpg --auto-key-locate wkd --locate-key security@flight-academy.app` *(available once the canonical site is hosted)*
- **Direct download**: <https://flight-academy.app/.well-known/security/pgp-key.asc> *(same)*

## Maintainer reply addresses

Mail sent to `security@flight-academy.app` is currently forwarded to the
maintainers' personal mailboxes. Replies will originate from one of the
addresses below and are PGP-signed where the content warrants it.

| Maintainer | Reply address | Verify against |
|---|---|---|
| Robert Shalders | `robert@shalders.co.uk` | [@ICreateThunder](https://github.com/ICreateThunder), the published fingerprint above |

If you receive a reply purporting to be from a Flight Academy maintainer
from any address not listed here, treat it as suspicious and confirm via
[GitHub Private Security Advisories](https://github.com/flight-academy-uk/flight-academy/security/advisories/new)
before responding.

## Scope

### In scope

- Source code in this repository
- Container images published to `ghcr.io/flight-academy-uk/`
- Helm chart at `deploy/helm/` (once present) and bundled manifests
- Documentation that describes security-relevant configuration

### Out of scope

- Misconfiguration of self-hosted deployments where documentation correctly describes secure defaults
- Issues in third-party dependencies (report upstream; we track via Dependabot and `cargo-audit`)
- Vulnerabilities requiring physical access to tenant infrastructure
- Issues in the hosted Flight Academy service stemming from a customer's own integrations or misconfiguration
- Social-engineering attacks against maintainers
- Denial-of-service via resource exhaustion requiring resources not normally available (e.g. ≥1 Tbps L4 floods)

## Bounty

Flight Academy does not currently operate a bug bounty programme. Reporters acting in good faith receive:

- Acknowledgement in the security advisory (unless anonymity is preferred)
- Listing in `SECURITY-HALL-OF-FAME.md` once it exists
- Maintainer-side good will

## Threat model

A detailed threat model will live at [docs/security/threat-model.md](docs/security/threat-model.md). Summary of the trust boundaries we defend:

- **Multi-tenancy** — assume one tenant attempts to read or modify another tenant's data. Defence: PostgreSQL row-level security + ABAC at the application layer + database role separation.
- **Credential compromise** — assume a maintainer's GitHub credentials are stolen. Defence: org-wide 2FA, signed commits, branch protection rulesets, no long-lived secrets in CI, OIDC for cloud access.
- **Supply chain** — assume an upstream dependency is malicious or compromised. Defence: `cargo-deny` allowlist, `cargo-audit` on every PR, CodeQL semantic analysis, SBOM published per release, container signatures via cosign keyless via Sigstore.
- **Hosted service** — assume hosting infrastructure is hostile. Defence: encryption at rest with per-tenant data-encryption keys (envelope encryption with KMS-wrapped DEKs), EU data residency, audit logging of every privileged action, crypto-shredding as a GDPR erasure backstop.
- **Self-host operator error** — assume operators will leave defaults insecure. Defence: secure-by-default configuration, refuse to start in unsafe modes, document every meaningful knob.

## No telemetry

Flight Academy contains no phone-home, no usage analytics, and no error aggregation that ships data to any party other than the operator. Self-hosted instances are observable only to their operators.

This is a project principle and a contribution requirement. Pull requests adding telemetry of any kind will not be merged. See [docs/architecture/ADR-001-platform.md](docs/architecture/ADR-001-platform.md) for the rationale.

## Acknowledgements

Security researchers who have responsibly disclosed vulnerabilities are credited in published security advisories on GitHub and in the project changelog.

## Licence

This policy is licensed under [AGPL-3.0](LICENSE) along with the rest of the project.
