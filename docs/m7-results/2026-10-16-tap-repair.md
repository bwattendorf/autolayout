# Round 24 (2026-10-16): tap repair

Branch `m7-escape`. Change: `autolayout route` gets a tap repair step after the portfolio and island repair (`crates/cli/src/main.rs`, `TAP_REPAIR_*`). Windows runner, KiCad 9.0.1, `AUTOLAYOUT_CORPUS=C:/Portcullis/controller/boards`, `tools/m7_batch.py`, all numbers measured. Compared with Round 22 (`out/m7r22_all`), which predates the rip-up retry commit 652ef7a, so part of the m1 change below is that commit, not this one (not separated).

## Cause found: R4.1 on net N$12 (fab/emu, emu/cinfront-candidate1, emu/loop-locked-candidate6)
N$12 joins R4.1 to U1.79 (an 80-pad part, 0.4 mm pitch). The search from U1.79 reached 5 grid states and stopped. U1.79 sits in a one-cell-wide lane (neighbouring pads 0.3 mm from its centre line block the rows either side). The lane's two exits were closed by plane-tap vias, placed first and never moved: a GND via 0.65 mm west and a +3V3 via 0.81 mm east, the taps of the cap C10 on the back side under that pad row. Found by printing the search's reached set and the owner of the cells in the lane (temporary debug, not committed). The pad reported as unreached (R4.1) was the free end; the boxed-in pad was the other end of the net.

## Change
After the portfolio, for every pad of a net left incomplete (only if at most 12 pads are open), each plane-tap via within 6 pitches of the pad is forbidden for every net (`RoutingModel::block_via_near`, as the island repair does) and the board is routed again with the same settings; the replacement taps are checked the same way, up to 4 rounds. Kept only if it leaves fewer unconnected (then fewer violations); otherwise all sites are unblocked. fab/emu took two rounds (the first moved the taps to the next lane over and was not better).

## Full batch (`out/m7r24_all`, 37 boards, 0 errors) against Round 22
| metric | Round 22 | Round 24 |
|---|---|---|
| KiCad unconnected | 139 | **125** |
| nets complete | 4641 | 4642 |
| clearance / shorting / hole-to-hole | 122 / 5 / 4 | 122 / 5 / 4 |
| summed route_s | 1341.6 | 2154.9 |
No board is worse on KiCad unconnected, clearance, shorting or hole-to-hole. Boards that changed (KiCad unconnected, Round 22 -> 24):
- fab/emu 1 -> 0, emu/cinfront-candidate1 1 -> 0, emu/loop-locked-candidate6 1 -> 0 (the R4.1 cause); c4/3p5mil-candidate3 1 -> 0 (its open pad was U1.2; same repair, cause not examined separately).
- m1/candidate4 12 -> 10, m1/locked-candidate5 29 -> 25, m1/tp18-candidate3 15 -> 11: not isolated from commit 652ef7a (rip-up retry) or this change.
- m1/tp18-candidate3 nets complete 93 -> 88 while unconnected fell 15 -> 11: fewer complete nets but fewer open pads. Not investigated.
- Time: summed route_s up 60% (1342 -> 2155 s), single runs under unequal load, repair rounds add attempts (fab/emu 18 -> 44 s, cinfront 53 -> 120 s). Not separated from the rip-up retry.

## Not shown
- Whether the repair or the rip-up retry did the m1 improvements.
- One run per board. The remaining open boards are the same MCU-escape boards and g2i (2).
