# Roadmap

**Stage:** M1 — board model (read side done). Nothing places or routes yet.
**Updated:** 2026-10-12

## Headline numbers

| metric                                     | value       | measured / estimated                                         |
|--------------------------------------------|-------------|--------------------------------------------------------------|
| corpus boards round-tripping byte-exact    | 116 / 116   | measured 2026-10-12, `ci.sh` with `AUTOLAYOUT_CORPUS` set    |
| rotated-front / back pads meeting copper   | 96.6% / 100% | measured 2026-10-12, m1 fab board, `board` corpus test      |
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
- 2026-10-12 — record fix: the M0.1 ROADMAP/gaps updates were made with a
  Python string replace that read the files as cp1252, matched nothing and
  reported nothing. Rewritten with the editor. Lesson: an edit that can miss
  silently must be checked by reading the result back.
