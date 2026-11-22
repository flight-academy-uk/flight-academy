#!/usr/bin/env bash
# Lint GitHub Actions workflows: actionlint for correctness and expression
# safety, then scripts/check-action-pins.sh, which fails when a third-party
# action is not pinned to a full 40-character commit SHA (actionlint does not
# check pinning). CI runs the pin check in its Lint job.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: $2"; exit 2; }; }
need actionlint "install from https://github.com/rhysd/actionlint or your package manager (Artix/Arch: pacman -S actionlint)"

echo "=== actionlint ==="; actionlint

echo "=== action pins ==="; scripts/check-action-pins.sh
