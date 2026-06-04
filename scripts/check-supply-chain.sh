#!/usr/bin/env bash
# Run cargo audit (RustSec advisories) and cargo deny check (advisories +
# licenses + bans + sources per deny.toml). Mirrors what CI runs so
# contributors can catch issues before opening a PR.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: run 'cargo install --locked $1'"; exit 2; }; }
need cargo-audit
need cargo-deny

echo "=== cargo audit ===";  cargo audit
echo "=== cargo deny check ===";  cargo deny check
