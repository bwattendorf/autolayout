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
