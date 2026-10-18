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
  EM4095 (EM4095HMSO16B+). Decision 2026-10-18: buy a few as Digikey cut tape (2651-EM4095HMSO16B+CT-ND, about $4.25
  each per web search snippets) and hand-solder; TME is the fallback. The pcbparts tool only shows the 2500-piece reel
  record, so confirm the cut-tape listing and stock on the Digikey page before ordering. See
  out/sourcing/em4095_alternatives.md.
- Cost/availability swaps applied to every order BOM (tools/bom_swaps.json, tools/bom_patch.py; lookups 2026-10-18):
  BAT54S-7-F C83574 -> hongjiacheng BAT54S C7420333 (preferred, no extended fee); THVD1450DR C2671361 -> SN65HVD75DR
  C57928 (3-3.6 V; all 14 boards that carry it feed pad 8 from the 3.3 V rail; datasheet pinout and firmware bus rate
  not checked); SRN6045TA-4R7M C2044594 -> Sunlord SWPA6045S4R7MT C78804 (same 6x6 body, 3.3 A rated vs 4.5 A);
  TL3305AF160QG C2886899 -> SHOU HAN TS4538TP C398052 (same 4.5x4.5x3.8 body, 2.5 N vs 1.6 N, 50k vs 500k cycles).
  Land patterns of the last two were NOT compared with their datasheets; same-size parts were chosen so no board is
  re-placed or re-routed. Not applied: relay swap (Songle for Omron G5LE; the user wants long-life relays for soak
  use, so the Omron stays); AP63203 fixed 3.3 V for AP63200 (its FB pin works differently, so the feedback divider
  on the schematic would change; not a BOM-only swap). Scan: out/sourcing/cheaper_parts_scan.md.
- AP63201 -> AP63203 (user decision 2026-10-18; tools/fb_fix.py, fb_verify.py, fb_run.py, ltspice/ap63203_stage.cir).
  22 regulators on 17 boards run a 140k/44.2k divider = 3.33 V (vref 0.8 V; check = the m1 value); each becomes the
  fixed 3.3 V AP63203WU-7 (C780769) with both divider resistors deleted and pin 1 on the +3V3 net. Not converted:
  the 140k/10k regulators (12 V) and c2g U10 / g2c U2 (140k/37.4k, 3.8 V). Evidence: Diodes AP63203 EVB guide and
  SparkFun's open-source AP63203 schematic (pin 1 = FB wired to the 3.3 V output; pins 2 EN, 3 VIN, 4 GND, 5 SW,
  6 BST = the pad map of our boards; reference parts 4.7 uH, 10 uF in, 2 x 22 uF out, 0.1 uF boost = our values).
  Checks (all PASS, out/fb_verify.txt): edited netlist equals the netlist rebuilt independently from the original;
  board-vs-netlist differences unchanged; only the listed resistors removed. LTspice power stage (open-loop,
  out/ap63203_ltspice.log, assumed Rds(on)/derating, no vendor model): inductor peak 2.18-2.24 A at 2 A load
  (ripple 0.44-0.65 A p-p over Vin 9-30 V), output ripple 5-12 mV p-p with 100 % to 35 % capacitance; this does
  not test the IC's loop, start-up or current limit. Pre-existing and not investigated: c1 c2 c2g c4 cm e1 e2 el
  g2 io m2 r t have 50-210 differing pad groups between the board file and its netlist before the edit.
  Per-board LTspice (tools/ltspice_check.py, out/ltspice_all.txt): all 18 converted regulators PASS the only
  criterion (peak inductor current 2.24-2.25 A at 2 A load vs 0.8 x 5.5 A Isat; ripple 0.6 A p-p). Output
  capacitance on the 3.3 V net, from the netlists: 50-57 uF on m1 c1 c2 c4 cm g2n t; 36 uF on c2g e2 el g2 io m2 r;
  30 uF (3 x 10 uF) on g2i g2io g2o; 26 uF on e1. The Diodes EVM page quotes 2 x 22 uF as the recommendation; the
  minimum output capacitance and compensation of the AP63203 were NOT read, so stability on the 26-36 uF boards
  is unchecked (the old AP63201 boards had the same capacitors). Open-loop ripple numbers (14-25 mV) include
  start-up ringing and are not a regulation result.
  Datasheet read (Diodes DS41326 Rev 3-2, text via tools/pdf_text.py, Figure 21 via tools/pdf_page_png.py):
  Fig. 21 shows the AP63203 with FB (pin 1) wired straight to VOUT after the inductor, EN tied to VIN, 100 nF
  BST-SW, 10 uF in, 2 x 22 uF out = our board's wiring. Frequency 500 kHz for AP63200 and AP63201, 1.1 MHz for
  AP63203 and AP63205 (so the AP63200 stand-in matches the AP63201; the AP63203 is faster). Table 2: 3.3 V ->
  3.9 uH, 10 uF, 2 x 22 uF. Inductor guidance: dIL 30-50 % of 2 A, 2.2-10 uH, DC rating >= 35 % above load
  (our 4.7 uH gives 0.46-0.6 A = 23-30 %, Sunlord rated 3.3 A: inside the range). Output capacitor: "22 uF to 68 uF
  ceramic is sufficient" (no hard minimum stated); ours are 26-57 uF nominal on every converted board, so the
  question is DC-bias derating on the 26-36 uF boards (not measured; bench-check output ripple/load step on the
  first article). Input: ">10 uF ceramic is sufficient"; g2i g2io g2o have exactly 10 uF on the 12 V net.
  HS peak current limit 2.5 min / 2.8 typ / 3.1 max A (our worst 2.25 A at a 2 A load). VIN operating 3.8-32 V,
  absolute max 35 V DC, 40 V for 400 ms. The Portcullis schematic must get the same edit (delete the divider, tie
  FB to the output) or KiCad parity will flag it.
- SUPERCAP PROBLEM (KEMET FG0H474ZF datasheet read 2026-10-18): leads are flat 0.4 x 1.2 mm (LT1/LT2, +/-0.1), not
  round, so the 1.0 mm drill used in the boards is too small (worst-case diagonal ~1.4 mm; needs about 1.6 mm and a
  larger pad); and the listed "current at 30 minutes after charging" is 0.71 mA against the BOM requirement of
  <= 2 uA at 3.3 V (a 30-minute charging current is not a steady-state leakage, but the datasheet gives no
  steady-state figure; aqueous electrolyte, -25..+70 C, ESR 120 ohm at 1 kHz). Not yet fixed: pick another part.
  Replacement search 2026-10-18 (Digikey lookups measured; leakage figures are from search-result snippets of
  Eaton/Mouser pages because eaton.com timed out from here, so NOT datasheet-verified): KEMET FT0H474ZF (16.5 mm,
  5.08 mm pitch, same flat 0.4 x 1.2 mm leads, same 0.71 mA 30-minute spec, no steady-state figure in the series
  datasheet, $6.71, 23k stock); Eaton PB-5R0H474-R 5.0 V, 7 uA leakage, 1 ohm ESR, 14.5 x 17.3 x 9 mm horizontal,
  11.8 mm pitch, $5.93, stock 30; Eaton PM-5R0H474-R 5.0 V, 8 uA after 100 h, $12.27, stock 95; Eaton PHV-5R4V474-R
  13 uA after 100 h at 5 V, RoHS non-compliant, $7.96, stock 1955; Panasonic EEC-F5R5U474 end of life; Murata
  DMF3Z5R5H474M3DTA0 obsolete. No 0.47 F part found with <= 2 uA. Hold time to fall from 3.3 V to 1.8 V
  (estimate, C x dV / I): 7 uA ~ 28 h, 13 uA ~ 15 h, 2 uA ~ 4 days.
- RTC: RV-3028-C7 is out of stock everywhere checked; orders use RV-3032-C7 (C5127802), firmware change needed.
