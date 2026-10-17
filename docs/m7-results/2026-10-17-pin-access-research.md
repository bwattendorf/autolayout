# Pin access research (2026-10-17), for the U1 boxed-in pads on c4/candidate2

Measured here: c4/candidate2 leaves 12 pads open (KiCad-judged), all boxed in at or near U1
(QFN-80, 0.4 mm pitch; grid pitch 0.3098 mm). Widening the rip-up (also near joined pads, cap 14) changed
nothing (12 again, ~9 min/run); reverted. `tools/escape_exist.py U1` on c4 reaches 81/81 searches but ends
with 4-6 nets in conflict over ~5 cells (via-track 0.559 mm vs rule 0.573 mm), at 60 and at 150 iterations.
`route --fine` on c4 produced nothing in 900 s.

Found by search (not yet tried here; nothing below is measured on our boards):
- Chip detailed routers call this *pin access*. TritonRoute (UCSD, c377.pdf) precomputes per-pin access
  points/paths and picks a conflict-free combination per cell with dynamic programming, then routes on grid.
- Univ. of Milan slides (B1-2-Schulte): pins off-grid get a short precomputed off-grid path to an on-grid
  endpoint; the grid search starts from there.
- libreda `pin_access_oracle`: per pin, precomputed access locations, directions and vias.
- PCB practice (KiCad forum, SparkFun): put the grid origin on a pad and use a finer grid for fine-pitch parts.
- Escape routing (ICCAD'06, 10C_3.pdf): channel capacity between pads sets layers needed.

Implication: our seeded-copper path (`preconnected`) is the right interface. What is missing is a planner that
chooses access paths for all pads of one part jointly (DP over neighbouring pads / negotiated), in the router's
frame, and emits them as seeded copper before the grid search. Next: make escape_exist.py legal for U1 on c4
(grid-anchored lattice), then port to Rust as a per-part pin-access planner.

## Result: seeding the U1 escape plan made c4/candidate2 worse (measured)

- `escape_exist.py out/c4x/c4.kicad_pcb U1 --plane GND --plane +3V3 --grid 50 50 --sub 12 --margin 1.8
  --disc 0 --slack -0.003 --hist 1 --iters 250`: solved in 19 iterations (~1 min), 81/81 pads, no overlap
  (sub 9 left one via-via pair 0.009 mm short). KiCad 10 DRC (with the .kicad_pro beside the plan): no new
  track clearance/width violations; only signal vias vs unfilled plane zones (56+56), also an artifact.
- `route out/c4x/plan12.kicad_pcb --plane ... --explain` (plan as existing copper): best portfolio attempt
  22 unconnected (attempt 1), others 23/23/29; **worse than the 12 without the plan**. Explain: the joined
  ends (U1.11, .75, .77, .21, .38) reach only 2-15 cells, blocked mostly by *fixed obstacles* (the other
  escapes' copper and vias), not by other nets' claims.
- Reading: the plan ends every escape radially at the window edge with no knowledge of where the net goes,
  so the ends crowd each other and the router cannot leave them. Not yet verified which cells block an end.
- Not done / next: dump the blocked neighbours of one joined end (U1.11) to see whether it is clearance
  rounding at the lattice/grid mismatch or genuine crowding; then plan escapes toward each net's
  destination side instead of the nearest edge.
