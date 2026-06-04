# Operations documentation

This directory holds operational documentation referenced by the
[architecture decision records](../architecture/README.md) and the
[domain model](../design/domain-model.md). Operational specifics
(thresholds, runbook order, comms templates, conformance matrices,
template contracts) live here so the ADRs can remain architecturally
focused and date-stable.

## Status

All entries are currently planned and not yet written. They are not
blockers for accepting the architecture decisions; they become
required deliverables as their corresponding subsystems land.

## Queue

| Document | Cited by | Purpose |
| --- | --- | --- |
| [`compliance-roadmap.md`](./compliance-roadmap.md) | [ADR-016 §C](../architecture/ADR-016-compliance-baseline.md) | Specific dates and audit windows for certification pursuit (Cyber Essentials Plus, ISO 27001, ISO 27018, SOC 2 if applicable); kept here so date churn stays out of the ADR. |
| [`deployment.md`](./deployment.md) | [ADR-015 §F/§H](../architecture/ADR-015-csp-static-build.md), [ADR-002](../architecture/ADR-002-release-deployment.md) | Build pipeline specifics including the GitOps previous-deploy fetch mechanism, partition-add cadence, canary monitoring thresholds. |
| [`hardening.md`](./hardening.md) (private) | [ADR-004 §A](../architecture/ADR-004-defence-in-depth.md), [ADR-010 §F](../architecture/ADR-010-platform-operator-access.md) | Circuit-breaker thresholds, honeypot specifics, deception-layer identifiers. Private because contents inform attacker behaviour. |
| [`incident-response.md`](./incident-response.md) | [ADR-006 §J](../architecture/ADR-006-api-contract.md), [ADR-015 §I](../architecture/ADR-015-csp-static-build.md), [ADR-010 §J](../architecture/ADR-010-platform-operator-access.md) | CVE emergency-break runbook including order of operations, comms templates, tenant transparency notification flow. |
| [`pq-migration.md`](./pq-migration.md) | [ADR-013 §I/§J](../architecture/ADR-013-auth-keys.md) | Post-quantum primitives in use, library/provider readiness tracking, planned migration windows for hybrid Ed25519 + ML-DSA on artefact keys. |
| [`regulatory-watch.md`](./regulatory-watch.md) | [ADR-016 §A](../architecture/ADR-016-compliance-baseline.md) | UK CAA divergence from EASA tracking; ICAO Annex updates; jurisdictional changes affecting the applicable-by-law baseline. |
| [`rollback-runbook.md`](./rollback-runbook.md) | [ADR-002](../architecture/ADR-002-release-deployment.md), [ADR-003](../architecture/ADR-003-db-migrations.md) | Deployment rollback procedure including expand-contract migration reverse-roll, container-image rollback, GitOps state recovery. |
| [`self-host-conformance.md`](./self-host-conformance.md) | [ADR-016 §H](../architecture/ADR-016-compliance-baseline.md), [ADR-010 §H](../architecture/ADR-010-platform-operator-access.md), [ADR-017 §G](../architecture/ADR-017-outbound-http-ssrf.md) | Self-host accountability matrix: what the operator must do (NetworkPolicy, IMDSv2, KMS / `age` setup, audit-log review cadence) to claim conformance with the architecture's security and compliance posture. |
| [`../contracts/dpa-template.md`](../contracts/dpa-template.md) | [ADR-008 §F](../architecture/ADR-008-data-sharing-posture.md), [ADR-011 §H](../architecture/ADR-011-user-consent-grant.md), [ADR-016 §B](../architecture/ADR-016-compliance-baseline.md) | Data Processing Agreement template covering the hosted offering: controller/processor split, sub-processor list, sub-processor change notification, Art. 28 obligations. |

## How this list stays current

Any ADR that cites a TBD operations doc adds the entry here. When a
document is written, the row is updated with status `Written` and a
brief change-log note. When an ADR's referenced TBD is replaced by a
different doc, both ends update.

If an entry stays unwritten for longer than feels right, it's worth
asking whether the subsystem it documents has actually landed — if
not, the entry can wait; if yes, the doc is overdue.
