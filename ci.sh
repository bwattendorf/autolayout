#!/usr/bin/env bash
# The quality gate. Run before every commit; paste the last line.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
if ! out=$(cargo test --workspace -- --nocapture 2>&1); then echo "$out"; exit 1; fi
echo "$out" | grep -E "round-trip byte-exact|SKIPPED" || true
echo "ci: all checks pass"
