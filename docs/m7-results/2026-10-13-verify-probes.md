# Round 6: KiCad's points probed in the verifier (fab m1, Freerouting board)

- Commit: `431e2475d6d54e894e1cb24e300ef73a1ebb0fbd` (m7-router; later than d53fe79). KiCad 9.0.1 (KiCad facts are from the round-2 DRC report, not re-run). `cargo build --release` exit 0. No code or boards changed.
- Command (exit **1**, expected):
```
./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb   --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3   --probe 95.4582,95.2794 --probe 95.25,95.5 --probe 102.6653,97.9853 --probe 101.8218,98.3   --probe 100.3,95.1956 --probe 101.3518,93.357 --probe 101.7206,93.8032 --probe 101.1,94.85   --probe 50.425,50.425 > out/m7r2/verify_fr_round6.txt
```

## Whole output

```
out/m7r2/fr/m1_fab_routed.kicad_pcb: 1372 segments, 232 vias: unconnected 17, track_dangling 0, hole_to_hole 0, clearance 2, shorting 2, copper_edge 0, track_width 45, via_size 0
  zone fills read: 21 polygons (each island is a component of its net)
  clearance at (98.9032, 86.5243): +1V1 vs N$54: 0.0722 mm, rule 0.0889
  shorting_items at (98.9032, 87.1595): +1V1 vs N$54: -0.0704 mm, rule 0.0889
  clearance at (105.1313, 90.9107): N$85 vs +3V3_ADC: 0.0736 mm, rule 0.0889
  shorting_items at (105.4615, 91.2409): N$85 vs +3V3_ADC: -0.0705 mm, rule 0.0889
  unconnected at (101.6466, 110.1532): net GND part 2 of 15 (main part has 91 pads): pads [C36.2 U7.4 D27.1 C26.2 C19.2], 0 tracks, 0 vias, fills: F.Cu fill (100.111,102.116)-(111.909,114.342)
  unconnected at (97.3850, 101.6006): net GND part 3 of 15 (main part has 91 pads): pads [C6.2 C13.2 C11.2 C9.2], 0 tracks, 0 vias, fills: B.Cu fill (95.694,92.991)-(99.379,102.477)
  unconnected at (92.9522, 93.0561): net GND part 4 of 15 (main part has 91 pads): pads [SW2.2#2 C17.2 D15.1], 0 tracks, 0 vias, fills: F.Cu fill (89.796,90.041)-(96.179,95.450)
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
probe (95.4582, 95.2794):
  pad C4.1 net +3V3 on B.Cu -> net +3V3 part 1 of 4 (the main part)
  track net +3V3 on B.Cu -> net +3V3 part 1 of 4 (the main part)
  fill In1.Cu net GND -> net GND part 1 of 15 (the main part)
  fill In2.Cu net +3V3 -> net +3V3 part 1 of 4 (the main part)
probe (95.2500, 95.5000):
  pad U1.1 net +3V3 on F.Cu -> net +3V3 part 2 of 4
  pad C4.1 net +3V3 on B.Cu -> net +3V3 part 1 of 4 (the main part)
  track net +3V3 on F.Cu -> net +3V3 part 2 of 4
  fill In1.Cu net GND -> net GND part 1 of 15 (the main part)
  fill In2.Cu net +3V3 -> net +3V3 part 1 of 4 (the main part)
probe (102.6653, 97.9853):
  track net +3V3 on B.Cu -> net +3V3 part 3 of 4
  track net +3V3 on B.Cu -> net +3V3 part 3 of 4
  fill In2.Cu net +3V3 -> net +3V3 part 1 of 4 (the main part)
probe (101.8218, 98.3000):
  pad U1.38 net +3V3 on F.Cu -> net +3V3 part 2 of 4
  pad C8.2 net GND on B.Cu -> net GND part 1 of 15 (the main part)
  track net +3V3 on F.Cu -> net +3V3 part 2 of 4
  track net +3V3 on F.Cu -> net +3V3 part 2 of 4
  fill B.Cu net GND -> net GND part 1 of 15 (the main part)
probe (100.3000, 95.1956):
  pad U1.49 net +3V3 on F.Cu -> net +3V3 part 3 of 4
  track net +3V3 on F.Cu -> net +3V3 part 3 of 4
  track net +3V3 on F.Cu -> net +3V3 part 3 of 4
  track net N$86 on In2.Cu -> net N$86: the only part
probe (101.3518, 93.3570):
  pad C1.1 net +3V3 on F.Cu -> net +3V3 part 4 of 4
  track net +3V3 on F.Cu -> net +3V3 part 4 of 4
  fill B.Cu net GND -> net GND part 1 of 15 (the main part)
  fill In2.Cu net +3V3 -> net +3V3 part 1 of 4 (the main part)
probe (101.7206, 93.8032):
  pad C2.2 net GND on B.Cu -> net GND part 1 of 15 (the main part)
  fill B.Cu net GND -> net GND part 1 of 15 (the main part)
  fill In1.Cu net GND -> net GND part 1 of 15 (the main part)
  fill In2.Cu net +3V3 -> net +3V3 part 1 of 4 (the main part)
probe (101.1000, 94.8500):
  pad U1.47 net GND on F.Cu -> net GND part 15 of 15
  track net N$58 on In1.Cu -> net N$58: the only part
probe (50.4250, 50.4250):
  no copper within 0.005 mm
```

## Table: KiCad's pairs against the probes

KiCad's item at each point is from `fr/m1_fab_routed_drc.rpt` (Top Layer = F.Cu, Bottom Layer = B.Cu). "Found" is the probe line that matches KiCad's item (same kind, net and layer).

| pair | point | KiCad item | found by probe | verifier part |
|---|---|---|---|---|
| +3V3 #1 | (95.4582, 95.2794) | Track +3V3, Bottom, 0.7810 mm | track +3V3 on B.Cu | +3V3 part **1 of 4** (main) |
| +3V3 #1 | (95.25, 95.5) | Track +3V3, Top, 2.8909 mm | track +3V3 on F.Cu | +3V3 part **2 of 4** |
| +3V3 #2 | (102.6653, 97.9853) | Track +3V3, Bottom, 3.0387 mm | track +3V3 on B.Cu (two track lines) | +3V3 part **3 of 4** |
| +3V3 #2 | (101.8218, 98.3) | Track +3V3, Top, 0.3282 mm | track +3V3 on F.Cu (two track lines) | +3V3 part **2 of 4** |
| +3V3 #3 | (100.3, 95.1956) | Track +3V3, Top, 0.3456 mm | track +3V3 on F.Cu (two track lines) | +3V3 part **3 of 4** |
| +3V3 #3 | (101.3518, 93.357) | Track +3V3, Top, 0.1642 mm | track +3V3 on F.Cu | +3V3 part **4 of 4** |
| GND pad pair | (101.7206, 93.8032) | Pad 2 of C2, GND, Bottom | pad C2.2 net GND on B.Cu | GND part **1 of 15** (main) |
| GND pad pair | (101.1, 94.85) | Pad 47 of U1, GND, Top | pad U1.47 net GND on F.Cu | GND part **15 of 15** |
| zone anchor | (50.425, 50.425) | Zone [GND] on Top Layer and Bottom Layer (13 entries) | **no copper within 0.005 mm** | none |

## Are the two points of a pair in different parts?

| pair | parts | different? |
|---|---|---|
| +3V3 #1 | 1 and 2 | **yes** |
| +3V3 #2 | 3 and 2 | **yes** |
| +3V3 #3 | 3 and 4 | **yes** |
| GND pad pair (C2.2 / U1.47) | 1 (main) and 15 | **yes** |

## Zone anchor

At (50.425, 50.425) the verifier reports "no copper within 0.005 mm". KiCad names a GND zone on Top and Bottom Layer at that point (all 13 zone-vs-zone entries are reported there). So the 13 zone-vs-zone items cannot be matched to verifier parts by this probe. In the round-3 probe the fills' bounding boxes start at (50.425, 50.425) (e.g. B.Cu pts=11571 bbox=(50.425,50.425)-(157.575,119.575)), but a bbox corner is not necessarily inside the fill; I did not check.

## Probes that found no copper, or copper of a different net than KiCad names

1. **No copper:** probe 9, (50.425, 50.425).
2. **Wrong net for the KiCad item:** none. For all 8 pair points the item KiCad names is found with the same net and layer.
3. **Additional copper of another net at the same point (KiCad did not name it):**
   - (100.3, 95.1956): track net N$86 on In2.Cu -> N$86, the only part.
   - (101.1, 94.85): track net N$58 on In1.Cu -> N$58, the only part.
   - (101.8218, 98.3): pad C8.2 net GND on B.Cu (GND part 1) and fill B.Cu GND alongside the +3V3 F.Cu items.
   - (95.25, 95.5): pad C4.1 net +3V3 on B.Cu, part 1 (the main part), at the same point as the F.Cu +3V3 track in part 2.
   Listed as printed. I have not decided whether any of these matters.

## Not run

No routing, no DRC re-run, no read of verifier source, no probe of any points other than the nine given.
