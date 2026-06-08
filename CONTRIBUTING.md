# Contributing to Flight Academy

Thanks for your interest. This document describes how contributions land in the codebase.

## Code of Conduct

This project has a [Code of Conduct](CODE_OF_CONDUCT.md). By participating you agree to uphold it. Report unacceptable behaviour to `conduct@flight-academy.app`, or via [GitHub Private Security Advisories](https://github.com/flight-academy-uk/flight-academy/security/advisories/new) if you prefer a private channel.

## Developer Certificate of Origin (DCO)

Every commit must be signed off. This asserts you have the right to contribute it under the project's [AGPL-3.0](LICENSE) licence. Sign-off is done with the `-s` flag:

```bash
git commit -s -m "feat(auth): add passkey registration"
```

This appends a trailer to the commit message:

```text
Signed-off-by: Your Name <your.email@example.com>
```

The full DCO text is at <https://developercertificate.org>. By signing off you affirm that statement for the contribution. Pull requests with any unsigned commit will be rejected by the DCO check.

**Bot exemption:** automated dependency-update bots (Dependabot) are not required to sign off — they cannot add the trailer, and a dependency bump is a mechanical metadata change rather than a copyrightable contribution. Their commits are still GPG-signed by GitHub, so the cryptographic signing requirement below still applies to them.

**One-time git setup:**

```bash
git config --global user.name  "Your Name"
git config --global user.email "your.email@example.com"
```

## Signed commits (cryptographic, separate from DCO)

Commits to `main` must also be **GPG- or SSH-signed**. Sign-off (`-s`) is the DCO; signing (`-S`) is cryptographic proof of authorship. They are different things and both are required.

SSH signing is usually lowest-friction. Once configured:

```bash
git config --global commit.gpgsign true
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/your_signing_key.pub
```

Verify with `git log --show-signature`. GitHub's full setup guide: <https://docs.github.com/en/authentication/managing-commit-signature-verification>

## Conventional commits

PR titles must follow [Conventional Commits 1.0](https://www.conventionalcommits.org/en/v1.0.0/). The squash merge takes its commit message from the PR title, so the PR title is what lands in `main`'s history.

Format:

```text
<type>(<scope>): <short summary>
```

**Types:** `feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `ci`, `build`, `perf`, `security`, `style`, `revert`.

**Scopes** (suggested, not exhaustive): `auth`, `api`, `db`, `aviation`, `safety`, `bookings`, `web`, `mobile`, `deploy`, `docs`.

Examples:

- `feat(auth): add passkey registration endpoint`
- `fix(bookings): resolve conflict detection across DST boundary`
- `security(deps): bump tokio to 1.42 (CVE-2024-XXXXX)`

Breaking changes: append `!` after type/scope and include `BREAKING CHANGE:` in the body.

## Pull request process

1. **Fork** (external contributors) or **branch** from `main` (maintainers).
2. **Branch naming** is a soft convention: `feat/<short>`, `fix/<short>`, `chore/<short>`. Squash-merge means branch names don't appear in `main`'s history, so this is for your benefit, not enforced.
3. **Commit early, often.** Branch commits are squashed at merge — they don't need to be polished. The PR title is what becomes the squash commit message.
4. **Open a draft PR** as soon as you have something to discuss; mark "Ready for review" when complete.
5. **PR checklist** (in template):
   - [ ] DCO sign-off on every commit
   - [ ] Commits are signed
   - [ ] Tests added or updated
   - [ ] Rust: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check` pass — or run `scripts/check-all.sh` to invoke them in one pass alongside `cargo audit`, `gitleaks`, `typos`, `shellcheck`, and `actionlint`
   - [ ] Web (when MASH foundations land per [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) §O): Tailwind compile + CSS bundle size budget pass
   - [ ] Mobile: `flutter analyze`, `flutter test` pass (when touched, once `apps/mobile` lands)
   - [ ] User-facing changes documented (CHANGELOG `[Unreleased]`)
   - [ ] No telemetry / phone-home introduced
6. **Review**: minimum 1 maintainer approval. CODEOWNERS paths may require specific reviewers.
7. **Merge**: squash-only. A maintainer merges once approved, CI green, conversations resolved.

## What we're looking for

- **Bug fixes** — always welcome
- **Tests** — always welcome, especially around auth, multi-tenancy, and safety reporting
- **Documentation** — always welcome
- **Features** — discuss first in a GitHub Discussion or Issue before opening a PR. Saves both sides time if the design doesn't fit.
- **Architectural changes** — propose as an ADR ([docs/architecture/](docs/architecture/)). Architectural changes without an ADR will not be merged.

## What we're not looking for

- **Whitespace-only churn** or formatter reflows without functional change
- **Comments restating what code obviously does**
- **Features that add phone-home telemetry of any kind** — see the no-telemetry principle in [ADR-001](docs/architecture/ADR-001-platform.md)
- **Dependencies under licences incompatible with AGPLv3** — `cargo-deny` enforces this in CI

## Development setup

Current prerequisites (toolchain pinning via `rust-toolchain.toml` will follow once the toolchain decision settles; a `Justfile` will land alongside an end-to-end dev orchestration command):

- Rust 1.83+ (`cargo`, `rustc`, `rustfmt`, `clippy`)
- Docker (testcontainers for integration tests; Postgres 18 image)
- `gitleaks`, `typos`, `shellcheck`, `actionlint`, `cargo-audit`, `cargo-deny` — `scripts/check-all.sh` will tell you which are missing
- Bun 1.3+ — added back when the MASH foundations PR lands (Tailwind CLI compile per [ADR-020](docs/architecture/ADR-020-mash-frontend-architecture.md) §O); not required today
- Flutter latest stable (only when `apps/mobile` lands)

What currently runs locally:

```bash
cargo build --workspace
cargo test --workspace                              # spins up testcontainers
scripts/check-all.sh                                # mirrors the CI quality gate
```

Full setup guide will be at [docs/development/setup.md](docs/development/setup.md) when there is enough to write down beyond the above.

## Reporting bugs

Use the **Bug report** issue template. Include:

- Version (release tag or commit SHA)
- Environment (hosted instance / self-host, OS, container runtime if relevant)
- Steps to reproduce
- Expected vs actual behaviour
- Logs with secrets redacted

## Reporting security issues

**Do not** open public issues for security vulnerabilities. Use [GitHub Private Security Advisories](https://github.com/flight-academy-uk/flight-academy/security/advisories/new). Full policy at [SECURITY.md](SECURITY.md).

## Becoming a maintainer

See [GOVERNANCE.md](GOVERNANCE.md) for the formal path. Indicatively: sustained contribution over 3+ months, 5+ merged PRs spanning multiple areas, active review participation.

## Questions

GitHub Discussions is for open-ended questions. Issues are for actionable bugs/features. Sensitive matters via the [SECURITY.md](SECURITY.md) channels.

Thanks for contributing.
