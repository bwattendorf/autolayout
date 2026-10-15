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
