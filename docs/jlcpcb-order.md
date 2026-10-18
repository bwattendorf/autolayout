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

## Digikey / Mouser for the lines JLCPCB lacks (lookups 2026-10-18, single-unit, measured)
- ARJP11A-MASA-B-A-EMU2 (PoE magjack, through-hole): Digikey in stock 166, $9.01.
- Ag9912-LP (Silvertel PoE module): Mouser Ag9912LP stock 159, $9.50. Mouser also has AG9912-LPB, a
  surface-mount variant (stock 438, $9.00) that is a different part/footprint.
- BG95M3LA-64-SGNS (Quectel): Mouser stock 488, $37.13 (exact M3 part; the JLCPCB M4 swap is not needed).
- RV-3028-C7: Mouser lists the TA QA and TA QC variants with stock 0; the Digikey lookup found nothing. The
  RV-3032-C7 swap stays for now.
- Ag5412: Digikey 5061-AG5412-ND, stock 1165, $16.25, through hole.
- SE051C2HQ1/Z01XDZ: Mouser 771-SE051C2HQ1Z01XDZ, stock 14389, $5.84 (3x3 SMT; hand-solder or consign).
- IN-PI554FCH: Mouser 743-IN-PI554FCH, stock 55635, $0.77 (5x5 SMT).
- EM4095SO16B+: not found at Digikey or Mouser.
- 0.47 F supercap: no SMD part fits 6.3x7.7. Through-hole candidates (leakage NOT checked, datasheet needed;
  values from the lookup tool): Eaton PHV-5R4V474-R 5.4 V, 17.3x9.0x14.5 mm, pitch 11.8 mm, Digikey
  283-PHV-5R4V474-R-ND $7.96 stock 1955; KEMET FG0H474ZF 5.5 V, 14.5 mm dia x 18 mm, pitch 5.08 mm, Digikey
  399-10801-ND $4.38 stock 478. In the m1 freeplace board C39 is a LOCKED footprint (fixed by the board owner),
  so a through-hole swap needs the owner to confirm there is room.
- 2200 uF / 4700 uF at 10 mm: none exist. Best 10 mm: Panasonic EEE-FT1C102AP 1000 uF 16 V (Mouser, $1.59);
  470 uF 25 V Nichicon UWD1E471MCL1GS (Digikey, min order 500). Full data in out/sourcing/dkmouser.json.

## Parts with no JLCPCB match (all boards)
RTC RV-3028 (alt RV-3032-C7), 0.47 F supercap, 2200/4700 uF 10 mm caps (only lower capacitance fits),
BG95-M3 (BG95-M4 is a different regional variant), Silvertel Ag9912/Ag5412 PoE modules, ARJP11A PoE magjack,
IN-PI554 LED (WS2812B is a candidate), EM4095, SE051 (SE050 candidate). Details in `out/sourcing/alt_group*.json`.
Pin-compatibility ratings there are judgement, not datasheet checks.

## All boards under JLCPCB rules (2026-10-18; KiCad `pcb drc --refill-zones`, measured via tools/place_sweep.py)
Our own placement + route, then `fab_ready.py`: 0 unconnected and none of the absolute manufacturing DRC kinds on
every board below. DRC only, not a design review. Order files: `out/order/<board>/` (Gerbers zip, BOM with LCSC
numbers, SMD placement file, hand-source list); not uploaded anywhere.
- Standard JLCPCB vias (0.45/0.20): m1, e1, e2, c2, c4, cm, c2g, g2c, el, g2, t, g2n, g2i, g2o, g2io.
- Needs JLCPCB's smaller-via option (0.25/0.15, extra cost, price not looked up): c1 (1-2 nets open at the
  standard vias with every placement variant tried), io, m2, r (2 open each at the standard vias).
- emu: the service's board and BOM (not our placement).
- Footprint swaps made in copies of the inputs (tools/cap_fix.py, tools/swap_to_tht2.py): 0.47 F supercap ->
  through-hole 5.08 mm pitch / 14.5 mm body (drill 1.0, pad 2.0 are guesses); 4700 uF (e1, e2) -> CP_Elec_18x22;
  2200 uF (c1 c2 c4 cm) -> CP_Elec_16x17.5. The swapped parts were unlocked so the placer moved them.
- Hand-source: supercap (every board that has one), ARJP11A, Ag9912-LP, Ag5412, BG95-M3, SE051C2, IN-PI554,
  EM4095 (EM4095HMSO16B+; Digikey reel only, see out/sourcing/em4095_alternatives.md).
- Cost/availability swaps applied to every order BOM (tools/bom_swaps.json, tools/bom_patch.py; lookups 2026-10-18):
  BAT54S-7-F C83574 -> hongjiacheng BAT54S C7420333 (preferred, no extended fee); THVD1450DR C2671361 -> SN65HVD75DR
  C57928 (3-3.6 V; all 14 boards that carry it feed pad 8 from the 3.3 V rail; datasheet pinout and firmware bus rate
  not checked). Not applied: relay swap (Songle for Omron G5LE; the user wants long-life relays for soak use, so the
  Omron stays), tact switch (pad pattern not compared). Scan: out/sourcing/cheaper_parts_scan.md.
- RTC: RV-3028-C7 is out of stock everywhere checked; orders use RV-3032-C7 (C5127802), firmware change needed.
