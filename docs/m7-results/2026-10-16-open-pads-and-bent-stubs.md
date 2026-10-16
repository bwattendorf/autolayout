# Round 25 (2026-10-16): why the remaining pads are open, and bent stubs

Branch `m7-escape`. KiCad 9.0.1, `tools/m7_batch.py`, `AUTOLAYOUT_CORPUS=C:/Portcullis/controller/boards`, all numbers measured, one run per board. Baseline is Round 24 (`out/m7r24_all`, after the tap repair).

## Why the 101 open pads on the nine open boards are open (temporary debug in `search.rs`, not committed)
Boards: m1 candidate4 / candidate6 / doublesided-candidate4 / locked-candidate5 / tp18-candidate2 / tp18-candidate3, emu/candidate2, c4/candidate2, g2i. For each pad listed "unreached" in the route log: no landing cell at all, or the search from one side reached only a few grid states.
- **45 pads have no landing cell** (no search is even tried): almost all U1 pins on the m1 boards, plus g2i U2.10 and U2.11.
- **55 pads are boxed in**: the smaller side of the failed search reached 60 states or fewer (most 3-15). Blocked frontier cells of those regions: fixed obstacles (pad / hole clearance zones) 369, other nets' copper and stub corridors 231, plane-tap vias 34.
- **1 pad is not boxed in** (m1/locked-candidate5 U1.36, 331 states).
- So plane taps are about 6% of the blockers on these boards. Extending the tap repair to boards with more than 12 open pads would recover little. The rest is fixed geometry or other nets' copper. Not examined further: which of the 369 fixed cells are avoidable.

## g2i (2 open pads: U2.10, U2.11)
TSSOP-16 at 0.65 mm pitch (pads 1.475 x 0.4) on a 0.505 mm grid (Default track 0.25 / Power clearance 0.25, the routing uses the largest class clearance for every net). The same pad-pitch against grid-pitch mismatch as U1 on m1.
- The coarse Power clearance is not the cause: with the Power class clearance edited to 0.2 in a scratch copy (pitch 0.455 mm) 2 pads still open (a different two, U1.10 and U2.3).
- Two GND vias at (70.2, 75.25) and (70.2, 76.26), 0.87 mm from the back-side GND test point TP8.1, sit in the lane east of U2.10 and U2.11. The tap repair moved them four times (up to 38 sites forbidden) without improvement, so they are not the only blocker.
- The side boxed in was U2.11's (4 grid states reached; frontier: 6 fixed, 1 plane, 1 signal).

## Bent stubs (committed, off no board's result)
`Router::bent_cells`: when a pad has no straight-stub landing cell, try cells outward along its long axis (up to 4 pitches beyond the pad end, 2 pitches to the side): the stub runs along the axis, then a 45 degree jog onto the cell, both segments checked for clearance against foreign pads and corridor cells (`Costs::bent_stubs`, default on). Pads with a straight landing are not affected.
- Full batch (37 boards, 0 errors) against Round 24: **identical on every KiCad column on every board** (unconnected 125 / 125, nets complete 4642 / 4642, clearance / shorting / hole-to-hole 122 / 5 / 4 unchanged). No board improved and none got worse.
- g2i: portfolio attempt 1 got worse (4 unconnected, was 2), the best attempt stayed at 2.
- Kept under the rule agreed beforehand (keep if no board is worse). It has no measured benefit; remove `bent_cells` and `Costs::bent_stubs` if the code is not wanted.
- Not tried: choosing bent landing cells only where a short flood fill of free cells escapes the pad's pocket.
