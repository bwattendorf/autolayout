# Round 5: are the verifier's unconnected parts KiCad's parts? (fab m1, Freerouting board)

- Commit: `646d31229295588f1d49b0ec78ac234d983cd994` (m7-router; later than e85487b). KiCad 9.0.1 (KiCad facts are from the round-2 DRC report and the round-4 log, not re-run). `cargo build --release` exit 0. No code or boards changed.
- Command: `./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3 > out/m7r2/verify_fr_round5.txt` -> exit **1** (expected).

## Full verifier output

```
out/m7r2/fr/m1_fab_routed.kicad_pcb: 1372 segments, 232 vias: unconnected 17, track_dangling 0, hole_to_hole 0, clearance 2, shorting 2, copper_edge 0, track_width 45, via_size 0
  zone fills read: 21 polygons (each island is a component of its net)
  clearance at (98.9032, 86.5243): +1V1 vs N$54: 0.0722 mm, rule 0.0889
  shorting_items at (98.9032, 87.1595): +1V1 vs N$54: -0.0704 mm, rule 0.0889
  clearance at (105.1313, 90.9107): N$85 vs +3V3_ADC: 0.0736 mm, rule 0.0889
  shorting_items at (105.4615, 91.2409): N$85 vs +3V3_ADC: -0.0705 mm, rule 0.0889
  unconnected at (101.6466, 110.1532): net GND part 2 of 15 (main part has 91 pads): pads [C36.2 U7.4 D27.1 C26.2 C19.2], 0 tracks, 0 vias, fills: F.Cu fill (100.111,102.116)-(111.909,114.342)
  unconnected at (97.3850, 101.6006): net GND part 3 of 15 (main part has 91 pads): pads [C6.2 C13.2 C11.2 C9.2], 0 tracks, 0 vias, fills: B.Cu fill (95.694,92.991)-(99.379,102.477)
  unconnected at (92.9522, 93.0561): net GND part 4 of 15 (main part has 91 pads): pads [SW2.2 C17.2 D15.1], 0 tracks, 0 vias, fills: F.Cu fill (89.796,90.041)-(96.179,95.450)
  unconnected at (90.8141, 88.1855): net GND part 5 of 15 (main part has 91 pads): pads [R23.2 D7.1], 0 tracks, 0 vias, fills: F.Cu fill (88.500,83.274)-(92.171,88.934)
  unconnected at (107.6891, 109.6915): net GND part 6 of 15 (main part has 91 pads): pads [R58.2], 0 tracks, 0 vias, fills: F.Cu fill (107.113,108.721)-(108.673,109.865)
  unconnected at (98.9022, 105.5064): net GND part 7 of 15 (main part has 91 pads): pads [Y1.2], 0 tracks, 0 vias, fills: F.Cu fill (96.925,104.601)-(100.343,106.764)
  unconnected at (105.9660, 99.9211): net GND part 8 of 15 (main part has 91 pads): pads [D16.1], 0 tracks, 0 vias, fills: F.Cu fill (104.130,99.075)-(107.084,101.526)
  unconnected at (92.9522, 100.2561): net GND part 9 of 15 (main part has 91 pads): pads [SW2.2], 0 tracks, 0 vias, fills: F.Cu fill (90.903,99.395)-(94.626,100.903)
  unconnected at (98.7000, 98.3000): net GND part 10 of 15 (main part has 91 pads): pads [U1.61], 0 tracks, 0 vias, fills: F.Cu fill (95.874,96.367)-(101.526,100.435)
  unconnected at (104.3550, 98.3000): net GND part 11 of 15 (main part has 91 pads): pads [C16.2], 0 tracks, 0 vias, fills: F.Cu fill (103.481,96.969)-(105.000,98.485)
  unconnected at (102.0785, 87.8138): net GND part 12 of 15 (main part has 91 pads): pads [R14.2], 0 tracks, 0 vias, fills: F.Cu fill (100.620,86.647)-(102.739,88.399)
  unconnected at (100.0953, 76.3783): net GND part 13 of 15 (main part has 91 pads): pads [R4.2], 0 tracks, 0 vias, fills: F.Cu fill (94.674,74.890)-(104.575,86.312)
  unconnected at (76.3441, 74.5649): net GND part 14 of 15 (main part has 91 pads): pads [R40.2], 0 tracks, 0 vias, fills: F.Cu fill (74.210,74.058)-(76.866,76.600)
  unconnected at (101.1000, 94.8500): net GND part 15 of 15 (main part has 91 pads): pads [U1.47], 0 tracks, 0 vias
  unconnected at (95.2500, 95.5000): net +3V3 part 2 of 4 (main part has 45 pads): pads [U1.1 U1.38 U1.53 U1.54], 14 tracks with ends in (95.250,94.850)-(102.150,98.300), 1 vias, fills: In2.Cu fill (98.090,94.919)-(99.999,96.208)
  unconnected at (102.1500, 95.5000): net +3V3 part 3 of 4 (main part has 45 pads): pads [U1.45 U1.49 C8.1], 8 tracks with ends in (100.300,94.797)-(102.980,98.300), 1 vias
  unconnected at (101.3518, 93.3570): net +3V3 part 4 of 4 (main part has 45 pads): pads [C1.1], 2 tracks with ends in (101.352,93.241)-(103.918,93.357), 1 vias
```

## 2. GND pad pair (KiCad: C2.2 at (101.7206, 93.8032) and U1.47 at (101.1, 94.85))

| pad | where in the verifier output | other members |
|---|---|---|
| U1.47 | GND part 15 of 15, alone: pads [U1.47], 0 tracks, 0 vias, no fills | none; it is its own part |
| C2.2 | **not listed in any of parts 2-15** | the output does not print the main part's members, so I cannot read its part directly; by elimination it is in the main part (91 pads) |

So U1.47 is in a different part from C2.2 only by inference (C2.2 absent from every listed part). The verifier's location for part 15 is (101.1, 94.85), exactly KiCad's U1.47 point. C2.2's point (101.7206, 93.8032) does not appear in the output.

## 3. GND parts with no pad

| | count |
|---|---|
| GND parts listed (2 to 15) | 14 |
| of those with **no pad** (fills only, or fills + vias) | **0** (expected 13) |
| parts with pads AND a fill island | 13 (parts 2-14) |
| parts with a pad and no fill, tracks or vias | 1 (part 15, U1.47) |

Mismatch with the expectation: no GND part is fill-only. Every one of the 13 fill islands carries at least one pad (e.g. part 2: C36.2 U7.4 D27.1 C26.2 C19.2; part 6: R58.2; part 10: U1.61). KiCad's 13 zone-vs-zone entries are all reported at (50.425, 50.425); the verifier's 13 fill-bearing parts are at 13 different locations, none near it.

## 4. +3V3 pairs: which part's box holds each KiCad point

Boxes as printed: part 2 (95.250,94.850)-(102.150,98.300); part 3 (100.300,94.797)-(102.980,98.300); part 4 (101.352,93.241)-(103.918,93.357). The main +3V3 part has no box printed. Inclusive edges; the boxes are of track ENDS and are printed to 3 decimals.

| KiCad pair | point | in part 2 box | in part 3 box | in part 4 box |
|---|---|---|---|---|
| 1 | (95.4582, 95.2794) | yes | no | no |
| 1 | (95.25, 95.5) | yes (edge x) | no | no |
| 2 | (102.6653, 97.9853) | no | yes | no |
| 2 | (101.8218, 98.3) | yes (edge y) | yes (edge y) | no |
| 3 | (100.3, 95.1956) | yes | yes (edge x) | no |
| 3 | (101.3518, 93.357) | no | no | yes (edge, 101.3518 vs printed 101.352) |

| KiCad pair | two points land in two different parts? |
|---|---|
| 1 | **No**: both only in part 2's box |
| 2 | Point 1 in part 3 only; point 2 in parts 2 and 3. Boxes overlap, so a box test cannot say; it can be different parts (3 vs 2) or the same (3) |
| 3 | Point 1 in parts 2 and 3; point 2 in part 4 only: **different parts** either way |

Pair 1 is a mismatch with what a "two different parts" reading would predict. Boxes are bounding boxes of track ends, not the tracks themselves; I did not test any point against the tracks.

## 5. Listed by one tool and not the other

| item | verifier | KiCad |
|---|---|---|
| C2.2 pad (101.7206, 93.8032) | not listed (absent from parts 2-15) | listed |
| 13 GND zone-vs-zone entries at (50.425, 50.425) | not listed there | listed |
| 13 GND parts with pads + fill islands at (101.6466,110.1532), (97.385,101.6006), (92.9522,93.0561), (90.8141,88.1855), (107.6891,109.6915), (98.9022,105.5064), (105.966,99.9211), (92.9522,100.2561), (98.7,98.3), (104.355,98.3), (102.0785,87.8138), (100.0953,76.3783), (76.3441,74.5649) | listed | no entry at these locations |
| U1.47 (101.1, 94.85) | listed (part 15) | listed (pad pair) |
| +3V3 (95.25, 95.5) | listed (part 2) | listed (pair 1 point 2) |
| +3V3 (101.3518, 93.357) | listed (part 4) | listed (pair 3 point 2) |
| +3V3 (102.15, 95.5), part 3 (U1.45 U1.49 C8.1) | listed | not a KiCad item; falls inside the boxes of pair 2/pair 3 points |
| +3V3 pair points (95.4582,95.2794), (102.6653,97.9853), (101.8218,98.3), (100.3,95.1956) | not printed as locations; inside boxes per section 4 | listed |

Other observation, not interpreted: pad SW2.2 appears in two GND parts (part 4 [SW2.2 C17.2 D15.1] and part 9 [SW2.2]).
Counts: 14 GND + 3 +3V3 = 17 on both sides, as in round 4. Clearance (2), shorting (2), track_width (45) lines are unchanged from round 4.

## Not run

No routing, no DRC re-run, no check of any point against the actual track geometry, no read of the verifier source, and I did not look up C2.2's part by another route.
