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
| boards placed legally (KiCad DRC clean)    | 1 (m1)      | measured 2026-10-12, `autolayout place` + `kicad-cli pcb drc` |
| boards placed *routably*                   | unknown     | needs M6 (route it and count unconnected)                    |
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
| M8 | RF checks | intended RF (antenna keepouts, matching-network order and length, feed lines, stitching) and unintended RF (switch loops, crystals, fast-edge nets, cable-connected nets, plane gaps) reported on every corpus board, each limit cited to a datasheet or app note; the placer gets them as constraints |

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
- 2026-10-12 — M3.1: grouping, first part (`autolayout groups <pcb> <net>`).
  Part kinds from the part's own value units; supply nets from POWER-IN;
  ground = widest supply; MCU = ≥ 16 bidirectional pins; switching
  converters found from their inductor (switch node = the end on a non-rail
  net that reaches an IC; rail = supply or bulk-to-ground). On m1 all three
  converters match Portcullis's step-1 answer key exactly for inductor, boot
  cap and feedback divider, including U4, whose regulator is a second,
  locally re-declared copy of the AP63201 with SW/BST typed PASSIVE — a
  pintype-only rule misses it. r's fixed 5 V buck correctly has no divider;
  emu correctly has one converter (its other regulator is a linear LDO).
  Known over-reach: bulk caps shared by two converters on one input rail
  (m1 U3/U4 on +12V) are listed for both; separating them needs position,
  which is placement's job.
- 2026-10-12 — M3.2: one role per footprint; crystals by circuit shape.
- 2026-10-12 — M4.1: **first legal placer** (`autolayout place`). m1 from
  the staging board, 2.4 s, deterministic: 121/121 parts placed, 0 off
  board, courtyard overlaps only the 2 pre-existing locked terminal pairs.
  **KiCad DRC on the written file: 0 clearance violations** from placement
  (the first version had 1 — J11, a through-hole header, dropped onto C14
  on the back; through-hole parts now occupy both sides). Signal HPWL
  2 374 mm vs the service's 3 016 mm — **but that is not a win.** The top
  render shows everything movable packed into one blob between J4 and the
  relays, with whole regions empty; the service spreads parts with room to
  route. Short wires on an over-packed board is the classic analytic-
  placement artefact. Routability is unmeasured, so "better" is not claimed.
- 2026-10-12 — M6.1: routability harness. `autolayout strip` removes all
  copper (pcbnew's 9.0 SWIG binding breaks every later object after
  `board.Remove()`, so stripping is done in the lossless file layer), then
  `tools/route_check.py` pours identical planes (structural GND/+3V3 from
  `autolayout groups`), exports DSN, runs Freerouting 2.3.0 externally,
  imports, and lets **KiCad's DRC** count unconnected. Smoke test, our m1 v1,
  2 passes: 278 → 152 unconnected, 975 s (fan-out on the packed placement
  is slow). Router choice recorded: Freerouting is the fixed yardstick while
  placement improves; an own router (M7) comes later and is measured against
  it — Freerouting is GPL, so it can be run but never shipped inside
  autolayout.
- 2026-10-12 — record fix: the M0.1 ROADMAP/gaps updates were made with a
  Python string replace that read the files as cp1252, matched nothing and
  reported nothing. Rewritten with the editor. Lesson: an edit that can miss
  silently must be checked by reading the result back.
