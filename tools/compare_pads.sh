#!/usr/bin/env bash
# Compare two pad listings (autolayout pads / tools/kicad_pads.py output).
# Refuses a vacuous comparison: both files must have rows, and the same count.
# Exit 0 only when every row matches.
#   tools/compare_pads.sh A.tsv B.tsv
set -euo pipefail
a=$(grep -c . "$1" || true); b=$(grep -c . "$2" || true)
if [[ "$a" -eq 0 || "$b" -eq 0 ]]; then
  echo "VACUOUS: $1 has $a rows, $2 has $b rows -- nothing was compared"; exit 2
fi
if [[ "$a" -ne "$b" ]]; then
  echo "ROW COUNT DIFFERS: $1 $a, $2 $b"; exit 1
fi
d=$(diff --strip-trailing-cr "$1" "$2" | grep -c '^<' || true)
echo "$d of $a rows differ"
[[ "$d" -eq 0 ]]
