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

## Update: router-frame plan, validated by KiCad (same day, m1/candidate4 only)
- **Window size:** on the pad-aligned 0.05 mm lattice, windows of 2.5, 1.8 and 1.2 mm beyond U1's pad box all solve (10 to 13 iterations). The planner needs very little room.
- **Handoff problem:** straight lanes 0.4 mm apart leave no router grid node between them (cells within 0.31 mm of a foreign lane are blocked), so about 4 in 10 lanes would have no node to continue from. The plan therefore has to end exactly on router grid nodes.
- **Router-frame lattice** (`--grid 50 50 --sub N`: lattice step = grid pitch 0.3098 / N, anchored at the router's grid origin, non-plane escapes must end on a node in the outer band): N = 6 did not solve at any window size (pad rows 7.75 steps apart, quantisation); **N = 9 and N = 12 solve at 1.8 mm** (15 and 16 iterations).
- **Hole-to-hole:** the first N = 9 and N = 12 outputs had one KiCad `hole_to_hole` warning each (two +3V3 vias of the same net 0.07 mm apart; same-net vias were allowed to overlap). Fixed: every via keeps hole-to-hole spacing from every other via. `out/exist/plan1.kicad_pcb` (N = 9, 1.8 mm): 303 segments, 27 vias; KiCad 9.0.1 DRC after refilling zones: clearance 14 (the same pre-existing pairs), 40 `track_dangling` (the ends at the window edge, expected), nothing else new, unconnected 380 -> 367.
- Command: `python tools/escape_exist.py BOARD U1 --plane GND --plane +3V3 --grid 50 50 --sub 9 --margin 1.8 --disc 0 --slack -0.003 --hist 1 --iters 250 --out PLAN.kicad_pcb` (grid origin 50 50 and pitch from `autolayout model`: outline minimum and track + clearance + max_error).

## Next (not started)
1. **Router: start a net from existing copper of the same net.** `RoutingModel` already reads existing segments and vias as obstacles owned by their net. Missing: in `Router::route_net`, a pad with same-net copper attached (a segment end inside the pad's copper, followed through segments and vias) counts as joined, and the free ends of that copper that sit on a grid node seed the search tree. Plane-net pads that reach a via that way need no tap (`tap_plane`).
2. **Run on m1/candidate4:** route `plan1.kicad_pcb` (after the plane pour and refill, as in `tools/m7_batch.py`) and count KiCad unconnected; today 10. That is the test of the whole idea. Everything else only matters if this helps.
3. **If it helps:** port the planner to Rust (fine-pitch part detection, the lattice solve), behind a switch, kept only if the 37-board batch shows no board worse. If it does not help: record it and stop.
