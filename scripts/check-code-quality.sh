#!/usr/bin/env bash
# Run typos (spell check), cargo fmt --check, cargo clippy -D warnings, and
# ShellCheck against the public scripts. Mirrors what CI runs so contributors
# can catch issues before opening a PR.
set -euo pipefail
cd "$(dirname "$0")/.."

need () { command -v "$1" >/dev/null 2>&1 || { echo "$1 not installed: $2"; exit 2; }; }
need typos      "run 'cargo install --locked typos-cli'"
need cargo      "install Rust via https://rustup.rs"
need shellcheck "install from https://github.com/koalaman/shellcheck or your package manager (Artix/Arch: pacman -S shellcheck)"

echo "=== typos ==="; typos
echo "=== cargo fmt --check ==="; cargo fmt --check
echo "=== cargo clippy --workspace --all-targets -- -D warnings ==="
cargo clippy --workspace --all-targets -- -D warnings
echo "=== shellcheck scripts/*.sh ==="; shellcheck scripts/*.sh
