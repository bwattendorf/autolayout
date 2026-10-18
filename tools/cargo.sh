#!/usr/bin/env bash
# Run cargo with ~/.cargo/bin on PATH, so a command line can be `bash tools/cargo.sh build ...`
# with no `export PATH=...;` prefix (a prefix like that makes Claude Code ask permission each time).
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.." || exit 1
exec cargo "$@"
