#!/usr/bin/env bash
# Lint GitHub Actions workflows for correctness and common security issues
# (shell injection in `${{ ... }}`, unpinned action SHAs, missing `permissions:`,
# etc.). actionlint auto-discovers .github/workflows/. Mirrors what CI runs.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: $2"; exit 2; }; }
need actionlint "install from https://github.com/rhysd/actionlint or your package manager (Artix/Arch: pacman -S actionlint)"

echo "=== actionlint ==="; actionlint
