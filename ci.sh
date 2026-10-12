#!/usr/bin/env bash
# The quality gate. Run before every commit; paste the last line.
#
#   AUTOLAYOUT_CORPUS=C:/Portcullis/controller/boards ./ci.sh
#
# Without AUTOLAYOUT_CORPUS the corpus tests skip and say so. Without KiCad the
# KiCad cross-check skips and says so. A skip is printed, never silent.
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")"

KICAD_PY="${KICAD_PY:-C:/Program Files/KiCad/9.0/bin/python.exe}"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
if ! out=$(cargo test --workspace --release -- --nocapture 2>&1); then echo "$out"; exit 1; fi
echo "$out" | grep -E "round-trip byte-exact|pad angles|fuzz:|SKIPPED" || true

# KiCad cross-check: autolayout edits a board, then KiCad and autolayout must
# agree on every pad position in the edited file (CLAUDE.md rule 3 — KiCad,
# not our own model, grades the edit). Poses are deliberately awkward: a 135°
# front part, a 30° back-side part, the MCU at 45°.
if [[ -z "${AUTOLAYOUT_CORPUS:-}" ]]; then
  echo "SKIPPED: KiCad cross-check (AUTOLAYOUT_CORPUS not set)"
elif [[ ! -x "$KICAD_PY" ]]; then
  echo "SKIPPED: KiCad cross-check (no KiCad python at $KICAD_PY)"
else
  cargo build --release -q
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  src="$AUTOLAYOUT_CORPUS/quilter/freeplace/m1/m1.kicad_pcb"
  ./target/release/autolayout move "$src" "$tmp/moved.kicad_pcb" R8 50 40 135 C4 60 45 30 U1 54 35 45 >/dev/null
  ./target/release/autolayout pads "$tmp/moved.kicad_pcb" >"$tmp/ours.tsv"
  "$KICAD_PY" tools/kicad_pads.py "$tmp/moved.kicad_pcb" >"$tmp/kicad.tsv"
  rows=$(wc -l <"$tmp/ours.tsv")
  if ! diff --strip-trailing-cr "$tmp/ours.tsv" "$tmp/kicad.tsv" >"$tmp/diff.txt"; then
    head -20 "$tmp/diff.txt"
    echo "FAIL: autolayout and KiCad disagree on pad positions after an edit"
    exit 1
  fi
  echo "KiCad cross-check: $rows of $rows pads agree after move/rotate (front, back, 45° MCU)"
fi

# Slow KiCad cross-checks (~20 min): every placed corpus board's courtyard
# overlaps against kicad-cli DRC. Run after touching courtyard geometry.
if [[ "${1:-}" == "--kicad-full" ]]; then
  if ! out=$(cargo test -p autolayout-grade --release --test kicad_drc -- --ignored --nocapture 2>&1); then
    echo "$out" | tail -30
    exit 1
  fi
  echo "$out" | grep -E "courtyard overlaps:|SKIPPED" || true
else
  echo "not run: slow KiCad DRC comparison (./ci.sh --kicad-full; last: 35/35 boards, 2026-10-12)"
fi
echo "ci: all checks pass"
