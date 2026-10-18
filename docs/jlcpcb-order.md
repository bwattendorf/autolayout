# JLCPCB first-order package (2026-10-18)

Status: files prepared, nothing ordered, nothing uploaded. Every price below is single-unit and
from the pcbparts lookup tool on 2026-10-18 (measured), except where marked estimate. Bare-board and
assembly prices are not known: get them from the JLCPCB quote page.

## m1 (our placement, `out/order/m1_ours/`)
- Board: 108.15 x 70.15 mm, 4 layers, 1.6 mm (from `m1-job.gbrjob`; the job file says finish "None": pick
  the finish on the order page).
- Checked: `place_sweep --rules jlc-std --boards m1` -> KiCad unconnected 372 -> 0, no new violations;
  `fab_ready.py`: no manufacturing DRC kinds. This is DRC, not a design review.
- Files: `m1_gerbers.zip`, `m1-jlc-bom.csv` (68 lines, LCSC numbers), `m1-cpl.csv` (151 placements;
  rotations are KiCad's, check the preview on the order page).
- Swaps applied (out of stock at the lookup): AP63201WU-7 (C2071044) -> AP63301WU-7 (C2158003, 3 A, 500 kHz);
  RV-3028-C7 (C3304278) -> RV-3032-C7 (C5127802; different register map, firmware change). Neither swap's
  pinout or datasheet has been checked.
- Not at JLCPCB: C39, 0.47 F supercap in a 6.3 x 7.7 SMD footprint. Searches found only through-hole
  0.47 F / 5.5 V parts (a Kyocera AVX SCMQ14F474PRBA0 listing; low leakage not verified) and a 47 mF SMD
  part (10x too small). Needs a footprint change or a different holdover design.

## emu (service board, `out/order/emu_service/` and `out/order/emu/`)
- Board: 152.55 x 127.15 mm, 4 layers. Gerbers re-exported with kicad-cli from the service's emu.kicad_pcb
  (not our placement: emu has no netlist for our placer).
- `emu-jlc-bom.csv`: 77 lines, every line has an LCSC part (parts only, one board about $62.5, 23 extended
  parts; estimate). `emu-cpl.csv` is the project's own.

## Parts with no JLCPCB match (all boards)
RTC RV-3028 (alt RV-3032-C7), 0.47 F supercap, 2200/4700 uF 10 mm caps (only lower capacitance fits),
BG95-M3 (BG95-M4 is a different regional variant), Silvertel Ag9912/Ag5412 PoE modules, ARJP11A PoE magjack,
IN-PI554 LED (WS2812B is a candidate), EM4095, SE051 (SE050 candidate). Details in `out/sourcing/alt_group*.json`.
Pin-compatibility ratings there are judgement, not datasheet checks.
