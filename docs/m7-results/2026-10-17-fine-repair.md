# Fine-lattice repair of the pads the grid router leaves open (2026-10-17)

`route --repair MARGIN` (new): after the grid router's best attempt, for each cluster of stuck regions
(where the side of the net that reached fewer grid cells sits, from the explain record), rip up
everything strictly inside a window (the lattice window less one pitch), keep the copper outside it as
fixed obstacles with true clearances, join each net's crossing points and pads inside again with the
fine-lattice PathFinder (`fine::repair_region`; the grid result is converted by `fine::grid_copper`),
and keep the result only if the verifier's unconnected count drops. The window grows by 3 mm (up to
+12) until each failed net inside it has something to join to. Refactor: `fine::negotiate` (job-based
PathFinder), `make_lattice`, `fill_obstacles`, `add_distance`, `stamp_via_block` shared with
`route_fine` and `plan_escapes`; the escape plan output of U1/c4 is byte-identical before and after.

## Measured on c4/candidate2 (`route --quick --iters 40 --repair 3`, router's verifier count)

- Baseline of that run: 9 unconnected.
- Windows around the far ends of long nets (R47.1, R51.1, U11.x, U3.7, ...): 1-16 nets re-routed,
  0 terminals unjoined, verifier 9 -> 9 every time (repair_region joins the terminals it was given,
  but the net's boxed-in side is elsewhere or its main piece does not cross the window).
- The dense window around U1: 38 nets re-routed; the fine PathFinder does not converge: 350-390
  conflicting cells at iteration ~133, 233 at iteration 214 of 250 (trace: `AUTOLAYOUT_FINE_TRACE=1`).
  An earlier run of the same window (before the stuck-region centring) ended 9 -> 13, discarded.
- Nothing from the repair was kept on c4; c4/candidate2 still has 9-12 open pads.

## Diagnostics added
`AUTOLAYOUT_FINE_TRACE=1` (iteration trace, per-job summary), `AUTOLAYOUT_REPAIR_DIFF=1` (unconnected
before/after), `Why::tree_at/pad_at` (where the stuck region sits).

## Not known
Why the fine PathFinder cannot settle 38 nets in the U1 window that the continuous planner
(`escape-plan`, 65/66 nets in 1.6 s) settles easily: the difference is that here every net has real
fixed crossing points and the neighbours' copper outside the window is fixed. Next: look at which cells
stay contested in the U1 window (as was done for the grid router).
