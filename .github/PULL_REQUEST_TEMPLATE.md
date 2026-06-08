<!--
Thank you for contributing to Flight Academy.

Read CONTRIBUTING.md before you open this PR if you have not already:
https://github.com/flight-academy-uk/flight-academy/blob/main/CONTRIBUTING.md

The PR title MUST follow Conventional Commits 1.0:
  <type>(<scope>): <short summary>
For example:  feat(auth): add passkey registration endpoint

The PR title becomes the squash-merge commit message in `main`'s history.
-->

## Summary

<!-- One to three lines. What does this change do, and why? -->

## Related issue

<!-- Use one or more of:
       Closes #123
       Fixes #456
       Refs #789
     Linking is mandatory for non-trivial changes. -->

Closes #

## Type of change

<!-- Tick all that apply. The first ticked type should match the PR title prefix. -->

- [ ] `feat` — new functionality
- [ ] `fix` — bug fix
- [ ] `chore` — tooling, deps, repo hygiene
- [ ] `docs` — documentation only
- [ ] `refactor` — code change with no functional difference
- [ ] `test` — adds or improves tests
- [ ] `ci` — CI / GitHub Actions / automation
- [ ] `build` — build system, containers, packaging
- [ ] `perf` — performance improvement
- [ ] `security` — security fix or hardening
- [ ] `style` — formatting / whitespace
- [ ] `revert` — reverts an earlier change

## Test plan

<!-- Describe how you verified this change. Both automated and manual matter.
     Reviewers will ask if this section is empty. -->

**Automated:**

- [ ] Unit tests
- [ ] Integration tests
- [ ] End-to-end tests

**Manual:**

<!-- Steps you ran locally. Be concrete. -->

## Checklist

<!-- All boxes must be ticked before a maintainer will review. If something
     genuinely does not apply, tick it and add a brief note explaining why. -->

- [ ] DCO sign-off on **every** commit (`git commit -s`) — see [CONTRIBUTING.md](../CONTRIBUTING.md#developer-certificate-of-origin-dco)
- [ ] Every commit is cryptographically **signed** (`git commit -S`) — see [CONTRIBUTING.md](../CONTRIBUTING.md#signed-commits-cryptographic-separate-from-dco)
- [ ] PR title follows **Conventional Commits 1.0**; breaking changes use `!` (e.g. `feat(api)!: …`)
- [ ] Tests added or updated where appropriate
- [ ] **Rust:** `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace` all pass locally
- [ ] **Web** (when MASH foundations land per [ADR-020](../docs/architecture/ADR-020-mash-frontend-architecture.md) §O): Tailwind compile + CSS bundle size budget pass
- [ ] **Mobile:** `flutter analyze` and `flutter test` pass (if `apps/mobile` touched)
- [ ] User-facing documentation updated (README, `docs/`, in-app copy)
- [ ] No new dependencies under licences incompatible with AGPLv3 (`cargo-deny` will enforce in CI)
- [ ] **No telemetry, analytics, or phone-home introduced** — this is a hard rule, see [SECURITY.md](../SECURITY.md#no-telemetry) and [ADR-001](../docs/architecture/ADR-001-platform.md)
- [ ] If this is an architectural change, an ADR exists in [`docs/architecture/`](../docs/architecture/) or is part of this PR

## Breaking changes

<!-- If this PR contains breaking changes, the PR title MUST use `!` (e.g. `feat(api)!: …`).
     Describe the break, migration path, and any deprecation timeline here. -->

None.

## Screenshots / recordings

<!-- For UI changes, attach before/after screenshots or a short screen recording.
     Delete this section if not applicable. -->

## Reviewer notes

<!-- Anything reviewers should pay particular attention to: trade-offs you
     considered, areas you're unsure about, follow-ups you have deliberately
     deferred. Be honest about rough edges — that helps review, it doesn't
     hurt it. -->

---

By submitting this pull request, I confirm I have read [CONTRIBUTING.md](../CONTRIBUTING.md) and that my contribution is licensed under [AGPL-3.0-only](../LICENSE).
