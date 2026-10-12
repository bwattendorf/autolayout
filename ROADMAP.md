# Roadmap

**Stage:** M2 — grader running and KiCad-verified for overlaps; M3 grouping next. Nothing places or routes yet.
**Updated:** 2026-10-12

## Headline numbers

| metric                                     | value       | measured / estimated                                         |
|--------------------------------------------|-------------|--------------------------------------------------------------|
| corpus boards round-tripping byte-exact    | 116 / 116   | measured 2026-10-12, `ci.sh` with `AUTOLAYOUT_CORPUS` set    |
| rotated-front / back pads meeting copper   | 96.6% / 100% | measured 2026-10-12, m1 fab board, `board` corpus test      |
| pads agreeing with KiCad after an edit     | 523 / 523   | measured 2026-10-12, KiCad cross-check in `ci.sh`, m1        |
| boards where overlaps match KiCad DRC      | 35 / 35     | measured 2026-10-12, `./ci.sh --kicad-full`                  |
| m1 signal HPWL: service's return (the bar) | 3 016 mm    | measured 2026-10-12, `autolayout grade`                      |
| boards placed                              | 0           | —                                                            |
| boards routed                              | 0           | —                                                            |

## Milestones

| id | milestone | exit condition |
|----|-----------|----------------|
| M0 | Lossless file layer | every corpus `.kicad_pcb` parses and writes back byte-identical; malformed input returns an error, never a panic |
| M1 | Board model | footprints, pads (position, layer, net, pintype), nets, outline, keepouts readable; footprint move/rotate/flip written back and accepted by `kicad-cli pcb drc` with no new parse errors |
| M2 | Grader | `autolayout grade` reports, from the file alone: courtyard overlaps, parts off-board, ratsnest length, per-net HPWL, bypass-cap distance to the pin it serves, switch-loop size; cross-checked against KiCad DRC on the same file |
| M3 | Grouping | every footprint in exactly one group (fixed / converter / bypass / crystal+USB / per-connector field cluster / MCU support / residue), derived from connectivity and pintype only |
| M4 | Legal placer | rule-based initial placement + legalisation: no courtyard overlap, all on board, fixed parts untouched; beats the staging-grid input on every M2 metric |
| M5 | Optimising placer | wirelength/congestion optimisation (analytical or annealing, seeded); compared against the commercial return on the corpus |
| M6 | Routability | export DSN, route with Freerouting as an external process, report unconnected; placement scored by how well a router finishes it |
| M7 | Own router | (scoped once M5 results are in) |

## Progress log

- 2026-10-12 — repository created; CLAUDE.md, licences, M0 started.
- 2026-10-12 — M0.1: lossless parser/writer. 116 of 116 Portcullis board files
  round-trip byte-exact (largest 4.9 MB, 0.45 s release build incl. process
  start). `autolayout info` on `quilter/fab/m1/m1.kicad_pcb` counts 183
  footprints / 3112 segments / 321 vias, matching the counts Portcullis
  measured through pcbnew (`m1-layout-experiment-2026-10-10.md`).
- 2026-10-12 — M1.1: board model read side (footprints, pads, nets, outline,
  courtyard) and netlist reader. Pad transform verified against routed copper:
  on `quilter/fab/m1` 172/178 rotated front pads and 44/44 back pads meet a
  track end or via within 1 µm; the clockwise-rotation alternative scores
  4/178 and the extra-mirror alternative 18/44 back, so the convention is
  measured, not assumed (emu: 357/365, 30/30). Netlist join on m1 freeplace:
  0 netted pads without a pintype. Finding: no corpus board carries `pintype`
  itself.
- 2026-10-12 — M0.2: parser fuzzing. 20 000 seeded cases (noise, truncation,
  substitution, splicing): 0 panics, all 4 833 accepted inputs byte-exact; a
  1 000 000-deep nest is an error, not a stack overflow. **M0 closed.**
- 2026-10-12 — M1.2 (move + rotate; flip still open): `autolayout move`.
  Measured first: 7072/7072 rotated chip pads across 116 files store their
  angle in board terms, so a rotation adds its delta to every pad/text angle.
  Graded by KiCad: after moving R8 to 135°, back-side C4 to 30° and U1 to 45°
  on m1's freeplace board, `tools/kicad_pads.py` (pcbnew) and `autolayout pads`
  agree on 523 of 523 pads; the file diff is 92 lines, every one an `(at …)`;
  `kicad-cli pcb drc` loads it. The cross-check now runs in `ci.sh`.
- 2026-10-12 — M2.1: grader (`autolayout grade`): courtyard overlaps, parts
  off board, half-perimeter and MST-ratsnest wirelength split into signal and
  plane nets (> 16 pads, structural). Courtyard overlaps agree **exactly**
  with `kicad-cli pcb drc` on **35 of 35** placed corpus boards (101 pairs,
  0 false alarms, 0 misses; 20 min, `./ci.sh --kicad-full`). Getting there
  took three tries at one false positive, m1 R6/C22: boxes and convex hulls
  both fill the notch of C22's cross-shaped (concave) courtyard; real
  outlines fixed it but then missed equal-width ganged terminal blocks
  (collinear sides, corners on boundaries) until edge midpoints were probed.
  Both shapes are unit tests. First like-for-like numbers on m1: staging
  input 15 741 mm signal HPWL / 121 parts off board; the service's return
  3 016 mm / 0 off board / 2 overlaps (the known ganged blocks).
- 2026-10-12 — record fix: the M0.1 ROADMAP/gaps updates were made with a
  Python string replace that read the files as cp1252, matched nothing and
  reported nothing. Rewritten with the editor. Lesson: an edit that can miss
  silently must be checked by reading the result back.
