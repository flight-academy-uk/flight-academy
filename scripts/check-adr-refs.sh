#!/usr/bin/env bash
# Fail if any (ADR-NNN-…\.md) markdown link target in docs/architecture/ is missing.
set -euo pipefail
cd "$(dirname "$0")/.."
missing=$(grep -rhoE '\(ADR-[0-9]+-[^)#]+\.md' docs/architecture/*.md | tr -d '(' | sort -u \
          | while read -r f; do [ -f "docs/architecture/$f" ] || echo "$f"; done)
[ -z "$missing" ] || { echo "BROKEN ADR refs:"; echo "$missing"; exit 1; }
