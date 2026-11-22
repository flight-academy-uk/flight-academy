#!/usr/bin/env bash
# Fail when a GitHub Actions `uses:` reference is not a local action (./...)
# or owner/repo[/path]@<full 40-character commit SHA>. Plain grep, so CI runs
# it without installing anything; scripts/check-workflows.sh runs it locally.
set -euo pipefail
cd "$(dirname "$0")/.."

unpinned=$(grep -rnE '^\s*-?\s*\{?\s*uses:' .github/workflows \
  | grep -vE 'uses:\s*\./' \
  | grep -vE 'uses:\s*[A-Za-z0-9_.-]+/[A-Za-z0-9_./-]+@[0-9a-f]{40}(\s|,|\}|$)' || true)
if [ -n "$unpinned" ]; then
  echo "actions not pinned to a full commit SHA:"
  echo "$unpinned"
  exit 1
fi
echo "all actions pinned to a commit SHA"
