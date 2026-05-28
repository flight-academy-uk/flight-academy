# Governance

How Flight Academy is governed: who makes decisions, how they are made, and how the project evolves over time.

## Current state

Flight Academy is in pre-1.0 development, maintained by a single benevolent dictator (BDFL) — **[@ICreateThunder](https://github.com/ICreateThunder)** — who has final authority on technical and project decisions.

This is appropriate for the current scale. The intent is to transition to a multi-maintainer governance model once the project demonstrates sustained external contribution.

## Roles

### Contributors

Anyone who submits a pull request, opens a meaningful issue, or participates in a Discussion. No formal status; recognition through commit history and the changelog.

### Maintainers

Members of the `@flight-academy-uk/maintainers` GitHub team. Currently: @ICreateThunder.

Maintainers can:

- Review and merge pull requests
- Triage and label issues
- Manage milestones and releases
- Participate in security advisory response

Maintainers commit to:

- Reviewing pull requests where their CODEOWNERS path is touched, within 7 days
- Participating in security advisory response within the SLAs in [SECURITY.md](SECURITY.md)
- Upholding the [Code of Conduct](CODE_OF_CONDUCT.md)

### BDFL

A single maintainer with final authority. Currently @ICreateThunder.

The BDFL's role is to break ties and make decisions where the maintainer group is deadlocked or where a decision must be made faster than consensus allows. The BDFL is expected to use this authority sparingly and to justify decisions in writing — typically via an ADR.

## How decisions are made

### Day-to-day

Code changes go through pull request review. Maintainer approval + green CI + conversation resolution = merge. No formal vote required.

### Architectural

Significant architectural changes require an **Architecture Decision Record (ADR)**. "Significant" includes anything that:

- Affects the database schema in a non-additive way
- Changes the authentication or authorisation flow
- Alters multi-tenancy enforcement
- Touches the threat model in [SECURITY.md](SECURITY.md)
- Adds a new long-lived dependency on a service or vendor
- Changes the build, release, or signing process

ADR process:

1. Open a draft PR adding `docs/architecture/ADR-NNN-title.md` (next free number).
2. Use the template at [docs/architecture/ADR-template.md](docs/architecture/ADR-template.md).
3. Discussion happens on the PR.
4. Approval from at least one maintainer + a 7-day cooling period during which any maintainer may object.
5. Merge as `Status: Accepted`.

ADRs can be later marked `Superseded` by subsequent ADRs but are never rewritten — the original record stands for historical context.

### Disputes

If maintainers disagree and the disagreement is not resolved through discussion within 14 days, the BDFL makes the call and documents the reasoning, typically as an ADR or in the relevant PR.

### Code of Conduct violations

Reports go to maintainers via the private channels in [SECURITY.md](SECURITY.md). The maintainer group reviews and decides on action (warning, temporary suspension, ban). The BDFL may unilaterally act in cases requiring immediate response, with a written justification posted to the maintainers afterward.

## Becoming a maintainer

There is no fixed threshold. Indicative criteria:

- 5+ merged pull requests spanning multiple areas of the codebase
- Active review participation on others' pull requests
- Demonstrated alignment with project values (privacy by default, no phone-home, GDPR-first, secure-by-default)
- Sustained engagement over at least 3 months

Existing maintainers nominate; the BDFL ratifies. Newly-onboarded maintainers receive write access to the repository and team membership.

## Transitioning away from BDFL

When the maintainer group reaches 3+ active members beyond the BDFL, the BDFL may step down to peer-maintainer status. Governance at that point transitions to:

- Maintainer consensus for normal decisions
- Lazy consensus (no objection within 7 days) for non-controversial changes
- Formal vote (simple majority of active maintainers) for contested decisions

The current BDFL commits to making this transition when appropriate, not to retaining BDFL status indefinitely.

## Funding and commercial relationships

Flight Academy is run as a hosted SaaS commercial offering alongside the open-source project. The commercial entity:

- Does not receive special access to the codebase that contributors do not have
- Does not gate features in the OSS code — no "enterprise edition"
- Does not require contributors to assign copyright (DCO only)
- Contributes back to the OSS project as upstream-first

If the commercial entity needs functionality that conflicts with the OSS direction, that functionality will be built downstream or in a separate repository — never as a hidden gate in this codebase.

## Naming

"Flight Academy" is the name of this project. The term is descriptive (aviation + training + general), which means it is unlikely to be defensible as a registered trademark. We do not pursue trademark registration.

Forks, derivatives, and self-hosted instances may use the name "Flight Academy" if they wish; the AGPLv3 licence permits this and this Code does not grant — and could not grant — exclusive rights to the name.

Where confusion arises (e.g., a separate hosted service launches under the same name), we resolve it through community communication and clear labelling rather than legal action. This is an open-source passion project, not a brand defended via litigation.

If the maintainers ever launch a hosted offering under a more distinctive secondary name alongside "Flight Academy", that secondary name may be subject to its own naming policy, documented here at that time.

## Amendments

Changes to this document follow the ADR process. Open a PR; one maintainer approval + 14-day cooling period.

## Licence

This governance document is licensed under [AGPL-3.0](LICENSE) along with the rest of the project.
