#!/usr/bin/env bash
# Driver: run every pre-push check in sequence. First failing check halts the
# run with its own exit code.
set -euo pipefail
cd "$(dirname "$0")"
./check-adr-refs.sh
./check-supply-chain.sh
./check-code-quality.sh
./check-workflows.sh
