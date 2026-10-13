# M7.5: service placement vs our placement of m1, each routed by our router and by Freerouting

- Commit tested: `1e4fcf9915d72b4dca4fcd69a05adf9394b7b93a` (m7-router). KiCad 9.0.1. 2026-10-13. No autolayout code changed. No boards, no `.net` committed.
- Rules in force on all four rows: the fab m1 project (track 0.0889, clearance 0.0889, via 0.25/0.15).

## Step 1: the netlist

- `find /c/Portcullis -name "m1*.net"` -> **nothing**. The m1 board's netlist is named `controller.net`, not `m1.net`: `C:/Portcullis/controller/boards/m1-1door/hardware/kicad/controller.net` existed beforehand (age not checked).
- Ran, without `--fast`: `cd C:/Portcullis/controller/boards/_common && ./run-checks.sh` (it starts with `python bom_export.py`, regenerating every board's netlist). Exit **0**, last line "All checks pass." Afterwards `controller.net` is dated Oct 13 13:30 (172284 bytes). That regenerate also rewrote the tracked `*_skidl.log` files in Portcullis (they were already modified before I started); I did not review or commit those.
- I used `controller.net` as "m1.net". Not committed.

## Commands and exit codes

| step | command | exit |
|---|---|---|
| 2 | `autolayout place $F/m1.kicad_pcb <controller.net> out/m7r7/m1_ours_placed.kicad_pcb` (F = quilter/freeplace/m1) | 0 |
| 2 | `kicad-cli pcb drc --severity-all --output out/m7r7/placed_drc.rpt out/m7r7/m1_ours_placed.kicad_pcb` (the freeplace `m1.kicad_pro` copied beside it) | 0 |
| 3 | `autolayout strip` (51 copper items removed), fab `m1.kicad_pro` copied beside, `autolayout rules` | 0 / 0 |
| 4 | `"$KPY" tools/m7_check.py out/m7r7/m1_ours.kicad_pcb out/m7r7/router GND +3V3` | 0 |
| 4 | `autolayout verify out/m7r7/router_check.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3` | 1 (findings) |
| 5 | `"$KPY" tools/route_check.py out/m7r7/m1_ours.kicad_pcb out/m7r7/fr GND +3V3 10` (12m20s wall) | 0 |

`rules` printed: track 0.0889, clearance 0.0889, via 0.2500/0.1500 (both classes).

## Step 2: the `place` report (unedited)

```
flipped to the front: C32 C36 C31 C37 C38 C21
placed 121 part(s) in 5.37 s; unplaced: 0
before (C:/Portcullis/controller/boards/quilter/freeplace/m1/m1.kicad_pcb):
  footprints            183 (0 without courtyard)
  courtyard overlaps    2
      J1 / J4
      J2 / J3
  off board             121
  signal nets (102)     HPWL   15741.1 mm   ratsnest   13528.8 mm
  plane nets  (  2)     HPWL     541.5 mm   ratsnest    1037.2 mm  (> 16 pads)
after (out/m7r7/m1_ours_placed.kicad_pcb, read back):
  footprints            183 (0 without courtyard)
  courtyard overlaps    2
      J1 / J4
      J2 / J3
  off board             0
  signal nets (102)     HPWL    3051.2 mm   ratsnest    2617.9 mm
  plane nets  (  2)     HPWL     248.1 mm   ratsnest     639.9 mm  (> 16 pads)
```

KiCad DRC on the placed board (using the freeplace project's coarser rules): 389 violations + 390 unconnected items:
clearance 11, courtyards_overlap 2 (J1/J4 and J2/J3, the same two as the service board and as "before"), hole_to_hole 14, lib_footprint_issues 9, silk_edge_clearance 2, silk_over_copper 152, silk_overlap 199, unconnected_items 390. The first clearance example is `netclass 'Power' clearance 0.2500 mm; actual 0.2000 mm` between U1 pad 5 [N$16] and pad 6 [+1V1] at (95.25, 97.1)/(95.25, 97.5). Not looked at further.

## Step 6: the table (all numbers from KiCad's DRC / route_check JSON)

| row | unconnected | clearance | shorting | hole_to_hole | track_dangling | track_width | via_dangling | copper mm | vias | seconds |
|---|---|---|---|---|---|---|---|---|---|---|
| service placement + our router | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 3017.0 | 460 | 0.7 |
| service placement + Freerouting | 17 | 2 | 2 | 0 | 0 | 45 | 9 | 3305.6 | 232 | 377.4 |
| **our placement + our router** | **6** | **8** | 0 | 0 | 0 | 0 | 0 | 2977.8 | 464 | 16.2 |
| **our placement + Freerouting** | **68** | **4** | 0 | 0 | 0 | 22 | 7 | 3260.8 | 231 | 732.0 |

The two service rows are copied from `2026-10-13-fab-m1-round2.md`; not re-run. "seconds" for our router is the router's own line (`0.7 s`, `16.2 s`); for Freerouting it is `route_seconds` from the JSON (service row: round 2's 377.4; round 1 was 602.8). The two columns are therefore not the same quantity (router only vs. router + KiCad import/export).
Our-router rows report nets complete 104/104 (service) and **103/104** with 61 iterations and "nets dropped to make the result legal 1" (our placement). Other DRC kinds unchanged across all four rows: courtyards_overlap 2, silk_edge_clearance 2, lib_footprint_issues 9; silk_over_copper 103 on the service board and 152 on our placement; silk_overlap 199.

## First violation examples of each non-zero routing category

**Ours + our router: unconnected 6** (all net N$4):
```
@(96.9625, 87.8500) Pad 2 [N$4] of U2   <-> @(104.0000, 88.0000) Pad 1 [N$4] of TP12
@(79.4000, 96.5000) Pad 1 of SW1         <-> @(86.6000, 96.5000) Pad 1 of SW1
@(91.7100, 74.7000) Pad 2 of R2          <-> @(86.8200, 80.7500) Pad 1 of C20
+ U2.2 <-> C20.1, U2.2 <-> SW1.1#2, U1.26 <-> U2.2
```
**Ours + our router: clearance 8** (all "footprint J10 clearance 0.1300 mm", the USB-C connector's own override; all between a `VBUS` F.Cu track and a GND pad of J10):
```
@(97.8282, 109.7408) Track [VBUS] vs @(97.3, 110.98) PTH pad S1 [GND]: actual 0.1073 mm
@(97.6504, 109.7408) Track [VBUS] vs S1: 0.1073 mm
@(103.5178, 109.9186) Track [VBUS] 5.6896 mm vs S1: 0.1187 mm
@(96.4058, 109.5630) Track [VBUS] vs @(95.95, 110.0) PTH pad A12 [GND]: 0.1235 mm
@(96.2280, 109.5630) Track [VBUS] vs A12: 0.1235 mm
(+3 more, all VBUS vs S1, 0.1134-0.1187 mm)
```
**Ours + Freerouting: clearance 4** (also J10's 0.13 mm override): Track [VBUS] vs A12 0.1128 mm at (95.6073, 109.4927); Track [N$82] vs B12 [GND] 0.0950 mm at (89.4976, 111.1602), 0.1079 mm at (89.4976, 111.5875), 0.1287 mm at (93.3101, 115.4).
**Ours + Freerouting: track_width 22**: e.g. Track [N$73] on F.Cu actual 0.0666 mm at (76.1983, 114.9558); Track [+12V_IN] at (118.997, 108.1408). **via_dangling 7**: e.g. Via [+1V1] at (90.3203, 82.7770), Via [N$13] at (94.5387, 96.2309), Via [N$21] at (87.8329, 72.6834). **unconnected 68**: not itemised here; see `freerouting/m1_ours_routed_drc.rpt`.

## Verifier vs KiCad on the new board (ours + our router)

| category | KiCad | verifier |
|---|---|---|
| unconnected | 6 | 6 (net N$4, 7 parts, each a single pad; all six in the same places as KiCad's pad pairs) |
| clearance | **8** | **0** |
| others | 0 | 0 |

The verifier does not report the 8 J10 clearance violations. J10's 0.13 mm footprint-level override is in the KiCad report; whether the router or verifier reads footprint overrides I did not check.

## DSN check (step 5)

`fr/m1_ours.dsn` carries `(width 88.9)`, `(clearance 88.9)`, `(clearance 22.225 (type smd_smd))`, `(via "Via[0-3]_250:150_um")`: the fab rules were in force.

## What failed / what I changed / not run

- Nothing failed. No autolayout code or script changed. No Portcullis file was edited by me; the regenerate rewrote generated files there (above).
- `m1.net` does not exist under that name; `controller.net` was used (see step 1). I did not check that it matches the freeplace board's netlist other than by `place` reporting 0 unplaced parts of 183.
- Not run: the service-placement rows (copied), `--kicad-full`, any repeat for variance (single run each), any layout-quality judgement of the placement, any look at the 68 Freerouting unconnected items individually.

## Raw files

`2026-10-13-m7-5-placements/`: `place_report.txt`, `placed_drc.rpt`, `verify_router.txt`, `router/*_drc.rpt|_route.log`, `freerouting/*_drc.rpt`, `freerouting/m1_ours_freerouting.log`. No boards.
