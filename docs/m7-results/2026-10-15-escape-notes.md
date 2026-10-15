# Many-pin IC escape: first measurements (2026-10-15, branch m7-escape, no router change)

Question: why do the remaining unconnected pads sit on many-pin ICs? (Round 21 analysis: 121 of 139 touch a part with 20+ pads.)

## Measured on returned/m1/candidate4 (QFN-60, 0.4 mm pad pitch, pads 0.8 x 0.2; rules 0.1524 track / 0.1524 clearance; grid pitch 0.3098 mm)
Batch result (out/m7r22_all): 12 unconnected, 11 on U1 (U1.3, 7, 10, 14, 21, 24, 27, 36, 42, 44, 52 unreached).
For each U1 pad I computed its position along its edge modulo the grid pitch, relative to the grid origin (the outline minimum). 0.4 / 0.3098 = 1.29 grid cells per pad, so the phase steps 0.29 per pad and cycles.
- All 11 unreached pads have phase in 0.36 to 0.74 (the pad sits between grid lines).
- Of the 60 pads, the ones with phase 0.36 to 0.74 number about 23; about half of them are unreached. Pads with phase outside that band (about 37) are reached without exception here (one board, one run, counted by hand from the list, not a statistical test).
So the failures concentrate on pads whose centre is far from any grid line. The stub from such a pad must travel up to half a pitch to a landing cell, and its neighbours at 0.4 mm pitch are doing the same.

## Cheap experiment: shift the grid origin (temporary env var, reverted, not committed)
Verifier unconnected on `m1/candidate4` b_prepared (a board with fills present, so shift 0 is 17 here, not the 12 of the batch pipeline), shifts in fractions of a pitch applied to x and y:
0: 17, 0.1: 33, 0.2: 20, 0.3: 22, 0.4: 14, 0.5: 26, 0.6: 20, 0.7: 30, 0.8: 28, 0.9: 24 (measured, one run each).
Result: a swing of 14 to 33 from a sub-pitch shift of the whole grid. Shifting the grid is not a fix; the outcome is sensitive to alignment, which fits the phase finding above. A best-of-N over shifts would pick luck, and costs N routes.

## What this suggests (not done)
1. The cause is the grid/pad mismatch at fine pitch, not the plane or the portfolio. Any fix has to treat the pads of a fine-pitch part off-grid: stagger a dog-bone or via-in-pad fan-out per pad on an explicit lattice that matches the pad pitch, route from the fan-out vias on the grid, and keep the existing grid for everything else.
2. A first small step that can be tested on this one 24 s board: for a part whose pad pitch is below the grid pitch times 1.5, put the stub landing at the pad's own exit line at a fixed distance outward (a straight escape), and register it as an isolated stub, instead of searching for the nearest free grid cell.
3. Test on the targeted boards only (m1/candidate4, m1/doublesided-candidate4, emu/candidate2, c4/candidate2: 12, 15, 23, 12 unconnected, all U1-dominated) before any full batch.
Not measured: how many of the other boards' failures show the same phase pattern; whether a 2-layer QFN-60 can be escaped at all with these rules (that depends on via size 0.6096 against the 0.4 mm pitch and needs a dog-bone layout).

---

# Update: why the pads fail, and one ordering experiment (2026-10-15, measured on returned/m1/candidate4; temporary debug code and the ordering change are reverted, not committed)

## Why the unreached pads fail
A temporary counter in `Router::terminal` printed, each time a pad got no landing cell at all (`near_cells` empty), how many grid cells lie in reach and which filter removed them. Reading the 17 pads that ever came up empty (all on U1, QFN-60):
- reach 1.5 pitches: about 25 cells in reach; 20-24 of them fail `cell_free` (already held by another net's track, via halo or stub corridor, or a neighbouring pad's halo), 0-6 fail `stub_clear` (the straight stub would come too near a neighbouring pad), 0-2 fail the stub corridor test.
- reach 4.5 pitches: about 105 cells in reach; 73-95 are taken, 6-26 fail `stub_clear`, 2-8 fail the corridor.
So the pad is not unreachable on the bare board; its landing area is used up by other nets' copper by the time it is routed (the empties happen in the later passes). The block is `cell_free`, not the stub geometry.

## Experiment: route nets that touch an off-grid pad first
Change (reverted): in `order_signal_nets`, sort nets with any pad that has no grid cell inside it ahead of the rest, then the usual short-first order. Run: `tools/m7_batch.py` on m1/candidate4 only (the board the change is aimed at).
- Before (Round 22 batch): nets complete 87 of 99, KiCad unconnected 12, KiCad clearance 15, verifier clearance 1.
- After: nets complete 84 of 99, KiCad unconnected **18**, KiCad clearance 15, verifier clearance 1 (clearance unchanged); portfolio attempts all NOT clean (29 / 23 / 18 / 24 verifier unconnected).
Result: **worse**, so routing fine-pitch nets first is not the fix, and it moves the cost onto other nets. The full batch was not run, as agreed (the targeted board failed).

## What the evidence supports next (not done)
Reserve each fine-pitch pad's landing cell and stub corridor for its own net before any routing, as static ownership the other nets must route around (the stub corridor sharing of two adjacent 0.4 mm pads is the hard part: two nets own one cell, which the model's one-owner `track` array cannot express). That means a reservation pass in `RoutingModel` plus a change in how `Router` builds its occupancy, i.e. a change to the core of the negotiation, not a local edit. Test on candidate4 first (about 25 s for one attempt) and then doublesided-candidate4, emu/candidate2 and c4/candidate2.

---

# Update 2: rip-up retry, and the flip-caps rule (2026-10-15, branch m7-escape; all measured with tools/m7_batch.py, KiCad DRC)

## Rip-up retry (crates/route/src/negotiate.rs, on by default, `--no-ripup` turns it off)
After the hard re-route of dropped nets, each net that still has a failed pad rips up the kept nets whose copper lies within the stub reach of that pad (at most 8), routes the failed net first and the ripped nets after it, and keeps the trial only if fewer pads fail over those nets. Deterministic (fixed net order), at most 40 trials per board.
- m1/candidate4: KiCad unconnected 12 -> **10**, nets complete 87 -> 89 of 99, KiCad clearance 15 -> 15, verifier clearance 1 -> 1.
- m1/doublesided-candidate4, emu/candidate2, c4/candidate2, m1/tp18-candidate2: **no change** on any column (retry improved 0 nets on each; route_s within noise: 15.2 -> 15.3, 359.9 -> 370.2, 254.9 -> 277.0, 64.9 -> 64.2).
- Why it helps little: a debug print on tp18-candidate2 (not committed) showed that in most trials the failed net does get routed first (its own pad connects) but the ripped nets then fail to re-route (e.g. before 4, after 11 failed pads). The board's front-side area around U1 is genuinely full, not merely mis-ordered.

## The flip rule (user's rule, 2026-10-15: if the board cannot be fully routed after about 10 renditions, move the non-electrolytic caps to the back)
- `autolayout flip <in> <out> --small-caps [--near "x,y;x,y" --count N]`: flips unlocked front-side capacitors (kind from the part value) whose pads span at most 3 mm to the back with the verified `edit::flip`; `--near` ranks them by distance to the failed pads, `--count` keeps the N nearest. Electrolytics are excluded by pad span, not by name. Prints the flipped references.
- `tools/m7_batch.py ... --flip-retry N`: if a board ends with KiCad unconnected > 0, route up to N renditions that flip 3, 6, 9, ... of the caps nearest the failed pads; keep the best by (KiCad unconnected, violations) and never one that adds violations. The chosen flips are in the new `flipped` column.
- Blanket flip of all 34 small caps on m1/candidate4: KiCad unconnected 19 (worse than 10).
- `--flip-retry 10` on m1/candidate4 (base with rip-up: 10 unconnected): rendition 1..10 (3..30 caps): 12, 11, **10**, 13, 12, 13, 13, 17, 17, 19 unconnected. No rendition beat the unflipped board (best was a tie at 9 caps, not kept), so the board stays unflipped.
- Result: **the rule runs, but flipping caps does not fix m1/candidate4**; the unconnected pads stay on U1. The caps are not what blocks U1's landing area. Not tried: the other U1-heavy boards with the rule (m1/doublesided-candidate4 15, emu/candidate2 23, c4/candidate2 12, m1/tp18-candidate2 12); flipping other part kinds (resistors).

## Gate
`bash ci.sh`: exit 0, "ci: all checks pass". The full batch has not been run, as agreed.
