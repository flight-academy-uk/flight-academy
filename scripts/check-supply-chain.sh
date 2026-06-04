#!/usr/bin/env bash
# Run cargo audit (RustSec advisories), cargo deny check (advisories +
# licenses + bans + sources per deny.toml), and gitleaks (secrets / PII).
# Mirrors what CI runs so contributors can catch issues before opening a PR.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: $2"; exit 2; }; }
need cargo-audit "run 'cargo install --locked cargo-audit'"
need cargo-deny "run 'cargo install --locked cargo-deny'"
need gitleaks   "install from https://github.com/gitleaks/gitleaks/releases or your package manager (Artix/Arch: pacman -S gitleaks)"

echo "=== cargo audit ===";   cargo audit
echo "=== cargo deny check ==="; cargo deny check
echo "=== gitleaks dir ===";  gitleaks dir .
