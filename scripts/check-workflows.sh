#!/usr/bin/env bash
# Lint GitHub Actions workflows: actionlint for correctness and expression
# safety, plus a pin check that fails when a third-party action is not pinned
# to a full 40-character commit SHA. actionlint auto-discovers
# .github/workflows/; it does not check pinning itself.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: $2"; exit 2; }; }
need actionlint "install from https://github.com/rhysd/actionlint or your package manager (Artix/Arch: pacman -S actionlint)"

echo "=== actionlint ==="; actionlint

echo "=== action pins ==="
# Every `uses:` must reference a local action (./...) or owner/repo[/path]@<40-hex SHA>.
unpinned=$(grep -rhnE '^\s*-?\s*uses:' .github/workflows \
  | grep -vE 'uses:\s*\./' \
  | grep -vE 'uses:\s*[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}(\s|$)' || true)
if [ -n "$unpinned" ]; then
  echo "actions not pinned to a full commit SHA:"
  echo "$unpinned"
  exit 1
fi
