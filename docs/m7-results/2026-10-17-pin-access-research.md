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

## Follow-up: painting existing copper at the true clearance (tried, reverted)

- Cause checked by arithmetic, not by cells: plan tracks sit ~0.3068 mm apart (rules + 0.002) while the model
  blocks every cell within 0.3098 mm (pitch, includes max_error) of existing copper, so a cell between two
  planned escapes is claimed by both = BLOCKED for both, including each escape's own end node.
- Re-planning at the router's spacing (`--slack 0.0055`, sub 12) did not solve in 160 iterations (4 cells
  in conflict; the run was cut at 590 s).
- Router change (existing segments/vias painted with clearance minus margin) on out/c4x/plan12: best
  attempts 17-18 unconnected, 2 clearance findings, vs 22 before the change and 12 with no plan at all.
  Several joined ends (U1.11, U1.54, U1.2, U1.36) still reach only 4-11 cells. Reverted: it did not beat
  the unseeded router and it touches every board with existing copper.
- Conclusion so far: a radial escape plan does not beat the grid router on c4/candidate2. Not known: what
  actually pens the ends that reach 4-11 cells (needs a cell dump/picture of one pocket, e.g. U1.54).

## c4/candidate2 follow-ups, all measured on c4 only (KiCad-judged best portfolio attempt, open pads)

Baseline (committed router, no flags): 12.
- Tight-pads-first net order: 19 (reverted).
- `--hist 80 --present 60`: 14 (flags kept, defaults unchanged).
- Capacity (`autolayout capacity --part U1`): 64 nets cross each ring, room for ~300 (strict) at every ring
  0.6-6 mm: the room is there, the router does not use it.
- Contested cells at the end of negotiation (`AUTOLAYOUT_CONTEST=1 ... --explain`): mostly stub cells shared
  by adjacent U1 pins (U1.43/44, 54/55, 38/39, 79/80, 21/22, 16/17/18).
- Bug found: `Shape::bbox()` of a rect/roundrect is the bounding *circle's* square, so `bent_cells` (and the
  ratio test in the new lane planner) never saw a long pad as long; bent stubs have only ever fired for
  oval pads. Fixed with `shape::pad_extent` (size + angle). Alone: still 12 on c4 (attempt 1 22 -> 18).
  NOT yet measured on the other boards: this turns bent stubs on for every rectangular long pad.
- `--lanes` (joint grid-line choice per row of long pads, DP, `RoutingModel::plan_lanes`, 305 pads on c4):
  15, worse than 12; off by default. The unreached pads still sit in pockets ringed by other nets' copper
  (`AUTOLAYOUT_MAPS=1 ... --explain` prints the cell maps).
- Open: why PathFinder leaves ~14-20 contested cells forever with 5x capacity. Next: per-net view of one
  pocket (U1.65) across iterations, and whether the pockets exist at iteration 1.

## Fast loop and sweeps on c4/candidate2 (router's own verifier count, one attempt, `route ... --quick`)

`--quick`: one attempt (stub reach 4.5, shared corridors), no island/tap repair: **41 s** instead of 6-10 min.
- `--iters`: 10 -> 15 open, 25 -> 11, 30/35 -> 11, 40 -> 9, 45/50 -> 9, 60/80 -> 12 (non-monotonic: the
  result depends on which iteration is "best" at legalisation).
- `--lanes` at 25 iters: 14. `--hist 60`: 11. `--present 15`: 11. Finer grid (max_error 0.001, pitch 0.3058): 15.
- Legalisation that drops the fewest nets (conflict-graph, most-conflicted first) instead of first-come:
  13 / 9 / 12 at 25 / 40 / 60 iters, no better; reverted.
- `--seed N` (jitters shortest-first order by up to +50%, splitmix64 of net and seed; 0 = off), 40 iters:
  seeds 1-8 -> 13, 15, 14, 10, 14, 11, 10, 14 open. Restarts alone spread 9-15; none reaches 0.
- Dump filter: stub-vs-stub overlaps are not conflicts in the model; the real residual conflicts are
  track-vs-track between neighbouring U1 pins' escapes (e.g. U1.65/U1.66, U1.72/U1.73 on the left edge).
- Service copper near U1 (candidate2, same 6/6 mil rules): within 4 mm 44 F.Cu + 24 B.Cu segments, 11 vias;
  ours 31 + 23, 13 vias. Not informative about how it escapes (its segments are short, ours merged).

## Escape planner ported to Rust (`autolayout escape-plan`, `fine::plan_escapes`) and tried on c4/candidate2

- `escape-plan PCB --part U1 --out NEW [--margin 1.8 --sub 12 --iters 250 --slack -0.003 --toward MM]`: windowed
  fine lattice around one part, every signal pad to the window's outer line (grid nodes), plane pads to a legal
  via, written as copper. Reuses `fine.rs` (a `run` with an optional escape scope).
- Speed, c4/U1, nearest-edge exits: **1.6 s, 65 of 66 nets** (U1.69, a +3V3 plane pad, finds no via), vs ~60 s
  for `tools/escape_exist.py`. KiCad 10 DRC (rules file beside the plan): only the 53 signal-via-vs-unfilled-
  zone findings seen before, nothing on the tracks.
- Routing from the plan (`route --quick --iters 40`, router's verifier count; unplanned baseline 9-11 open):
  nearest-edge exits 25; `--toward 3`: plan 39/66 nets in 594 s, route 28; `--toward 5`: 42/66, route 26;
  `--toward 8`: 62/66 in 148 s, route 34; `--toward 12`: 65/66 in 5 s, route 19.
- Conclusion: seeding a pre-planned escape makes c4 worse at every setting tried; directional exits are hard
  to solve (tight ones do not solve). The planner is a working tool; it is not wired into `route`.
- Not tried: planning with the other nets' destinations known (a windowed fine router with real goals),
  or fine-lattice repair of just the pads the grid router leaves open (needs existing copper in `fine.rs`).
