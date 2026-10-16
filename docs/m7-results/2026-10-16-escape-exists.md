# U1 escape: does a legal one exist? (2026-10-16, m1/candidate4 only)

Question from the plan: under this board's rules (track 0.1524, clearance 0.1524, via 0.6096 / drill 0.3048), does a legal escape for every U1 pad exist at all? Our grid router leaves 9 U1 pins open here (10 unconnected including R46.1).

## Method (`tools/escape_exist.py`, not part of the router)
A fine lattice (0.05 mm, aligned to U1's pads; the 0.4 mm pad pitch is 8 steps) over a window 3.5 mm beyond U1's pad box (280 x 280 cells), F.Cu and B.Cu, 8-direction moves, vias at any legal site. Every U1 pad with a net (54 of 70; the others are the unconnected paste/thermal pads) must reach the window edge on either layer; the 12 pads on plane nets (GND, +3V3) must reach a legal via site instead. Obstacles: every foreign pad on the board (exact rounded-box / oval / circle distance), the other escapes (track to track, via to track, via to via), with the project's clearance. Negotiated congestion (PathFinder) over all 54 at once, random net order each iteration. Pads of one net may touch each other's copper.

## Result
- With the exact rules plus 0.002 mm (`--disc 0 --slack -0.003`): **all 54 pads escape with no overlap, solved in 11 iterations (about 20 s).** 325 segments and 35 vias.
- KiCad 9.0.1 DRC on that copper written into a copy of the board (zones refilled): clearance 14 before and after (the same pre-existing J10 pad pairs), no new clearance, shorting or hole-to-hole violation. New: 42 `track_dangling` (the escapes end at the window edge, expected), and one `holes_co_located` (two +3V3 pads chose the same via site; the writer now writes it once). Unconnected 380 -> 368 (the 12 plane pads tied to their plane copper).
- With more allowance (0.03 mm lattice allowance, 0.005 mm margin) the search did not converge in 300 iterations; the leftovers were pairs at 0.539 mm against a 0.543 mm safety rule (exact rule 0.533 mm) and, with the larger allowance, a via pair at pads 37 and 38 of U1's right edge in a corridor of about 1.2 mm between U7 and C10. Removing U7 or C10 as a what-if also removed that conflict. So the allowance used in the check decides the outcome of this search; the answer above is for the exact rules.

## What this shows and does not show
- A legal escape exists for all of U1's pads on this placement, on two layers, in this window. The grid router leaves 9 of them open.
- It does not route the nets to where they go (only to the window edge, or to a via for plane nets), does not check that vias reach their plane's copper (pour pockets), and does not look beyond the window; the escapes also use up lanes the rest of the board then needs.
- Only m1/candidate4 was run. One run each; the search is randomised by net order only (seed 1), deterministic.
- Not examined: how much of the board's other routing the escapes block.
