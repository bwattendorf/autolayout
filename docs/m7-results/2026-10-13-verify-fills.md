# Round 4: fill-reading verifier vs KiCad, fab m1 round-2 boards

- Commit: `b997df059f39e06fd4fc2c1e11eea3e738dde482` (m7-router). KiCad 9.0.1 (KiCad numbers are the round-2 DRC reports, not re-run). `cargo build --release` exit 0. No code or boards changed.
- Boards: `out/m7r2/ours_check.kicad_pcb` and `out/m7r2/fr/m1_fab_routed.kicad_pcb`, each with its `.kicad_pro` beside it.

## Commands and exit codes

1. `./target/release/autolayout verify out/m7r2/ours_check.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3` -> exit **0**
2. `./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3` -> exit **1** (findings)

## Full verifier output, ours

```
out/m7r2/ours_check.kicad_pcb: 925 segments, 460 vias: unconnected 0, track_dangling 0, hole_to_hole 0, clearance 0, shorting 0, copper_edge 0, track_width 0, via_size 0
  zone fills read: 2 polygons (each island is a component of its net)
```
No `LIMIT:` line appeared. The "zone fills read" line says 2 polygons (round-3 probe: 1 + 1).

## Full verifier output, Freerouting

```
out/m7r2/fr/m1_fab_routed.kicad_pcb: 1372 segments, 232 vias: unconnected 17, track_dangling 0, hole_to_hole 0, clearance 2, shorting 2, copper_edge 0, track_width 45, via_size 0
  zone fills read: 21 polygons (each island is a component of its net)
  clearance at (98.9032, 86.5243): +1V1 vs N$54: 0.0722 mm, rule 0.0889
  shorting_items at (98.9032, 87.1595): +1V1 vs N$54: -0.0704 mm, rule 0.0889
  clearance at (105.1313, 90.9107): N$85 vs +3V3_ADC: 0.0736 mm, rule 0.0889
  shorting_items at (105.4615, 91.2409): N$85 vs +3V3_ADC: -0.0705 mm, rule 0.0889
  unconnected at (101.6466, 110.1532): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (107.6891, 109.6915): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (98.9022, 105.5064): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (105.9660, 99.9211): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (92.9522, 100.2561): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (98.7000, 98.3000): net GND split into 15 parts; nearest same-net track end is inf mm outside this pad
  unconnected at (102.1500, 95.5000): net +3V3 split into 4 parts; nearest same-net track end is -0.1000 mm outside this pad
  unconnected at (95.2500, 95.5000): net +3V3 split into 4 parts; nearest same-net track end is -0.1000 mm outside this pad
  unconnected at (99.0753, 110.9566): net +3V3 split into 4 parts; nearest same-net track end is -0.2700 mm outside this pad
```
No `LIMIT:` line appeared. "zone fills read: 21 polygons" (round-3 probe: 13 + 3 + 3 + 2 = 21).

## Table: verifier vs KiCad, per category

| category | ours KiCad | ours verifier | FR KiCad | FR verifier |
|---|---|---|---|---|
| unconnected | 0 | 0 | 17 | 17 |
| clearance | 0 | 0 | 2 | 2 |
| shorting | 0 | 0 | 2 | 2 |
| track_width | 0 | 0 | 45 | 45 |
| track_dangling | 0 | 0 | 0 | 0 |
| hole_to_hole | 0 | 0 | 0 | 0 |
| copper_edge | 0 | 0 | 0 | 0 |
| via_dangling | 0 | not implemented | 9 | not implemented |

## Table: unconnected per net (Freerouting board)

| net | KiCad | verifier |
|---|---|---|
| GND | 14 (13 zone-vs-zone + pad pair C2.2/U1.47) | 14 (`net GND split into 15 parts`) |
| +3V3 | 3 track pairs | 3 (`net +3V3 split into 4 parts`) |
| total | 17 | 17 |

## Differences (listed, not reconciled)

1. **Where the GND findings land differs.** KiCad puts 13 of the 14 at (50.425, 50.425) (zone fragments, all reported at that one point) and 1 at the pad pair (101.7206, 93.8032) / (101.1, 94.85). The verifier prints GND locations at (101.6466, 110.1532), (107.6891, 109.6915), (98.9022, 105.5064), (105.9660, 99.9211), (92.9522, 100.2561), (98.7, 98.3). None is near (50.425, 50.425) and none is at the C2.2 / U1.47 pair. The verifier's per-net count matches KiCad's; its locations do not.
2. **Only 6 GND lines are printed for 14 GND findings, and 9 unconnected lines for 17 in total.** I do not know whether the output is capped or whether it lists one line per something else; I did not read the source.
3. **+3V3 locations: one of six points matches.** KiCad pairs: (95.4582, 95.2794)-(95.25, 95.5); (102.6653, 97.9853)-(101.8218, 98.3); (100.3, 95.1956)-(101.3518, 93.357). Verifier: (102.15, 95.5), (95.25, 95.5), (99.0753, 110.9566). Only (95.25, 95.5) coincides with a KiCad item (the second item of the first pair). (102.15, 95.5) and (99.0753, 110.9566) are not at any KiCad-listed item. The handoff said the verifier prints one location per extra component, possibly a different item of the same component, so those may still be the same components; I have not checked that.
4. **Verifier text for the findings is odd and quoted as printed:** "nearest same-net track end is inf mm outside this pad" on all 6 GND lines, and "-0.1000 mm", "-0.1000 mm", "-0.2700 mm outside this pad" on the three +3V3 lines. I do not interpret negative or infinite distances.
5. **Clearance and shorting coordinates agree with KiCad's DRC items:** clearance (98.9032, 86.5243) and (105.1313, 90.9107, the track item of KiCad's (105.2183, 91.1377) via/track pair); shorting (98.9032, 87.1595) and (105.4615, 91.2409, the track item of KiCad's pair). Values: 0.0722 / 0.0736 mm clearance, -0.0704 / -0.0705 mm shorting, as printed. Not a difference, listed for completeness.
6. **via_dangling:** KiCad 9 on the Freerouting board, verifier does not implement it. Known and unchanged.
7. **Round-2 verifier on this board for contrast:** unconnected 0, clearance 4 (shorting counted as clearance). Now unconnected 17, clearance 2, shorting 2.

## Not run

No routing, no KiCad DRC re-run, no reading of verifier source, no re-run for variance.
