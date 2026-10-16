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

---

# Update 3: what surrounds U1, and rerouting (2026-10-15, m1/candidate4 after the rip-up retry; 10 KiCad unconnected)

## What occupies the front layer around U1
ASCII map of F.Cu (0.2 mm cells) from the routed board, plus a count. Within 2.5 mm of U1's pad box there are 32 nets, 133 F.Cu segments (120 mm) and 82 vias in the window; every F.Cu net there has a pad on U1 (no through traffic on the front layer), so U1's own fan-out fills the ring. The unreached pads (3, 7, 10, 14, 21, 24, 27, 36, 40, 52) are pads in the middle of an edge run whose neighbours' escape tracks run parallel and close to the pad ends (e.g. a long track at y = 102.5 mm under the bottom row, 0.35 mm from the pad ends, blocks pads 21/24/27 from going down). Nothing else (caps, other ICs) sits within the escape zone: the nearest parts are 3+ mm away, which also explains why flipping caps did not help.

## Does rerouting help? No (measured)
`--reroute 10` (route the whole board again with the nets that failed so far routed first, accumulating the set): all ten renditions were worse than the unrerouted result of 10: verifier unconnected 18, 23, 16, 19, 15, 14, 19, 19, 19, 19. It is the same finding as the earlier "off-grid nets first" experiment: giving the failing nets first pick takes cells from other nets that then fail. The code was reverted (not committed).

## Order of fallbacks agreed with the user
reroute (tried, no) -> flip small caps (built, no help on candidate4) -> placement adjustment -> more layers only when everything else has been used up (an explicit, reported option, since it changes the board design).

---

# Update 4: moving or rotating U1 (2026-10-15, m1/candidate4, scratch copies; U1 is a locked part, moved only in the copies; KiCad DRC, `out\m7e7_u1`)

28 variants of the board with `autolayout move` on U1: shifts of -0.2 to +0.2 mm in 0.1 mm steps in x and y (25, including 0,0), and rotations 90, 180, 270 degrees at the original position. Each was stripped, poured and routed through `tools/m7_batch.py` (rip-up retry on) and graded by KiCad.
- Unmoved (0,0): **10** KiCad unconnected, 89 of 99 nets.
- Best moved: +0.1/+0.2 mm: 11. Others: 12 to 22 for the shifts; rotations 90/180/270: **23 / 33 / 38**.
- No variant beat the unmoved board; none added a clearance, shorting or hole-to-hole violation (KiCad clearance 14-16 on all, shorting 0, hole-to-hole 0; the unmoved board has 15).
- Result: **moving or rotating U1 does not help m1/candidate4**, and the outcome swings by +/-10 for a 0.1 mm move (same sensitivity seen with the grid shift), so a local search on U1's pose would mostly pick noise. A search over the pose of other parts was not tried.

## Where this leaves the fallbacks
reroute: no. flip small caps: no (on this board). U1 shift/rotate: no. All three measured on m1/candidate4 only. The remaining unconnected pads are U1's own fan-out congestion on a 2-signal-layer stack; what is left is routing the fan-out better (the dog-bone/staggered-via escape idea in Update 1) or, last, more layers (agreed order: only when everything else is used up).

---

# Update 5: staggered-via fan-out and reserved landing cells (2026-10-15, m1/candidate4, QFN-60 at 0.4 mm pitch; rules 0.1524 track / 0.1524 clearance / 0.6096 via; `tools/fanout_proto.py`)

## A dog-bone fan-out of a whole edge is not possible under these rules (prototype, continuous geometry, no grid)
`tools/fanout_proto.py` searches, for U1's left edge (15 pads), a stub per pad (straight out, then one 45-degree jog) ending in a via, checking every clearance in continuous geometry (capsule/segment gaps against all pads, stubs and vias, with the router's 0.02 mm margin). Depth-first search, three pad orders, 120 s cap each:
- contiguous runs of **3, 4, 5** pads: a legal fan-out exists; **6**: none found in 40 s; **15**: none found.
Reasoning that matches it: a via needs about 0.53 mm of centre distance from a neighbouring track, adjacent lanes are 0.4 mm apart, and parallel 45-degree stubs at 0.4 mm pitch are only 0.28 mm apart (less than the 0.33 mm they need). So most pads cannot have their own via near the part. (With the 0.3 mm microvia in the project's defaults a fan-out would be possible, but that is a fab capability the rules do not allow.) Search is not exhaustive, so "none found" is not a proof for 6 to 15.

## Reserved landing cells (built, measured, reverted)
Idea: before routing, match the off-grid pads of each row monotonically to grid cells just outside the pad ends (dynamic programme, minimum sideways distance, strictly increasing cells), own those cells for their nets in `usage` so no other net takes them, and make `terminal()` use the reserved cell. Implemented in `Router::reserve_landings`, switched by `--reserve`.
- Rows found on U1: 4 (11, 7, 12 and 12 pads; the pads that already have a grid cell inside them are not in the rows); 7 to 9 of each row assigned, 0 candidates for 3 or 4 pads per row (their landing cells fall in neighbours' clearance).
- m1/candidate4 (rip-up retry on; unflagged result 10 unconnected, 89 of 99 nets), KiCad unconnected with `--reserve`: landing line 0.2 mm beyond the pad ends **26**; 0.35 mm **28**; 0.55 mm **15**; 0.8 mm **17**. Every setting is worse than not reserving.
- Reverted; not committed.

## State of the m1/candidate4 problem
Measured and found not to help on this board: reroute with failed nets first, flipping small caps, moving or rotating U1, reserving landing cells. Helps: the rip-up retry (12 to 10). Not tried: a router that works off-grid for the fan-out region (a local finer lattice), more layers (agreed as the last resort), moving other parts than U1 and the caps.

---

# Update 6: routing an inner plane layer as a signal layer (2026-10-15, m1/candidate4, KiCad 9.0.1, one run each; `out/drop_In1.Cu`, `out/drop_In2.Cu`)

`tools/m7_batch.py` with `M7_DROP_PLANES=<layer>`: that layer is not poured and the plane net is routed as an ordinary net, giving three signal layers. Baseline (both inner layers poured): 10 KiCad unconnected, 89 of 99 nets.

| Layer dropped | Plane still poured | Nets complete | KiCad unconnected | KiCad clearance | Route time |
|---|---|---|---|---|---|
| In1.Cu (GND routed) | In2.Cu = +3V3 | 86 of 99 | 13 | 14 | 50 s |
| In2.Cu (+3V3 routed) | In1.Cu = GND | 87 of 99 | 17 | 14 | 128 s |

- Both are worse than the two-signal-layer baseline, so a third signal layer does not fix the U1 escape on this board. Why is not tested: a guess is that the escape vias still have to leave the pad ring on F.Cu/B.Cu first.
- Clearance 14 equals the pre-existing count on the returned board (the unmoved baseline run shows 15), so no new violations were added.
- Single runs; the earlier U1 shift experiment swung by +/-10 for a 0.1 mm move, so differences of 3 to 7 here are within that noise. What the result supports is "no clear gain", not a ranking of the two layers.

## Update 6b: wider dog-bone offsets (same board, `tools/fanout_proto.py`, run 2026-10-15)
Straight-out length extended from 0.15-1.5 mm to 0.15-3.5 mm (212-215 options per pad, was fewer). Whole left edge (15 pads), three pad orders, 120 s cap each: **no legal fan-out found** (outside-in 384610 nodes, middle-out 610488, left-to-right 220606). Same conclusion as Update 5 and the same caveat: the search is capped by time, so "not found" is not a proof of impossibility.
