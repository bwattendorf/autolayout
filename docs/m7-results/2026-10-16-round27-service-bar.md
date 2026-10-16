# Round 27 (2026-10-16): all-KiCad-10 batch, the service's own open pads, escape survey

Commit tested for the router: `ed3622f` (same router as `babf09a`). All numbers measured, one run per board.

## All-KiCad-10 pipeline, 37 boards (`out/m7r27_k10`)
`tools/m7_batch.py` with KiCad 10.0.6's python (pour, refill) and `kicad-cli` (DRC); our router reads and writes version 20260206 files (nets by name). Against Round 25 (KiCad 9.0.1, `out/m7r25_all`, same router):
| metric | Round 25 (KiCad 9) | Round 27 (KiCad 10) |
|---|---|---|
| KiCad unconnected | 125 | 136 |
| nets complete | 4642 | 4640 |
| boards better / same / worse on unconnected | | 3 / 29 / 5 |
| hole-to-hole, dangling, track width, copper edge | 4 / 0 / 0 / 0 | 4 / 0 / 0 / 0 |
| clearance / shorting, raw report lines | 122 / 5 | 485 / 11 (repeats per layer) |
| clearance / shorting, once per item pair (`out/uniq_drc.py`) | 122 / 5 | 122 / 5 |
| verifier unconnected = KiCad unconnected | 37 of 37 | 37 of 37 |
Every difference is on the open boards: m1/candidate4 10 -> 20, m1/locked-candidate5 25 -> 29, m1/tp18-candidate3 11 -> 14, m1/tp18-candidate2 12 -> 13, g2i 2 -> 3; m1/candidate6 15 -> 12, m1/doublesided-candidate4 15 -> 13, emu/candidate2 23 -> 20. The 28 other boards are identical, including the ones at 0 unconnected. The made-up net codes of a name-style file (sorted names) change the order nets are tried in, and these boards swing by up to 10 on small changes (seen before with a 0.1 mm move of U1), so the +11 is not read as a regression of the router or of KiCad 10; it is not separated further. Summed route time 1821 s -> 2143 s under unequal load. The raw clearance count was the old counting of `m7_batch.py` (the batch started before the fix below).
Decision input: KiCad 10.0.6 gives the same verdict as 9.0.1 on identical copper (Round 26), and our verifier agrees with it on all 37 boards. Which one is the baseline is the owner's call; the numbers above show the all-KiCad-10 path changes only the order-sensitive open boards.

## `tools/m7_batch.py`: DRC counts once per item set
`drc()` now counts a violation once per distinct set of items it names. Checked on the saved reports: clearance 122 and shorting 5 on 37 boards for both KiCad 9.0.1 and 10.0.6 (raw 122/5 and 485/11). It also reduces some non-routing kinds that repeat in KiCad 9 (silk_overlap 199 -> 191 on m1/candidate4).

## The service's own open pads (KiCad 9.0.1 DRC on the returned boards as delivered, copied to `out/svc_drc`)
| board | service's returned copper, KiCad unconnected | ours (Round 25) |
|---|---|---|
| m1/candidate4 | 14 | 10 |
| m1/candidate6 | 15 | 15 |
| m1/locked-candidate5 | 34 | 25 |
| m1/doublesided-candidate4 | 13 | 15 |
| m1/tp18-candidate2 | 21 | 12 |
| m1/tp18-candidate3 | 21 | 11 |
| emu/candidate2 | 37 | 23 |
| c4/candidate2 | 13 | 12 |
| g2i (the `finish` file as given, 620 segments, partly routed) | 39 | 2 |
Total 207 against 125. On the nine boards where we leave pads open, the service's own returned board leaves as many or more on eight and fewer (2) on one (doublesided-candidate4). Caveats: these returns are older than the current netlists (CLAUDE.md, Round 18), so the open counts are against each file's own nets; g2i's `finish` file is the input state, not a service result; one DRC run each. Both sides use the project rules beside the board (minimum track 0.1524, via 0.6096 on the m1 candidates; the service's tracks there are 0.1525-0.1602 wide).

## Escape survey (`autolayout escape-survey`, `route::survey`, continuous geometry, pads only)
For each open pad on the nine boards (101, list from the route logs of `out/diag`): longest straight track along the pad's axis, away from its part, that keeps the clearance to every foreign pad (capped at 3.0 mm; tracks of other nets are not consulted, so an upper bound). 99 of 101 have 1.0 mm or more of room (many 3.0), one has 0.6, one is a connector pad the survey skips. So the open pads are not boxed in by pad geometry alone; the grid loses the room (no cell centre falls inside a narrow free corridor) and other nets' routed copper claims the rest.

## Tried and reverted (not committed): g2i U2.10 / U2.11
Bent stubs 8 pitches long and 3 to the side; a landing cell must lie in a free region of 300 cells (also applied to straight landing cells); rip-up retry looking at every pad of an incomplete net instead of only the unreached ones. Each left g2i at the same 2 open pads and the same portfolio rows (4 / 2 / 6 / 3 unconnected per attempt). Counting inside the router at the pad: U2.11's straight cells are in a large free region almost always; in 4 of about 230 terminal builds (late, when other nets have claimed the area) they are not and no bent cell exists either. Reverted to `babf09a`'s behaviour.

## Read together
The open pads are on placements the service itself did not finish, the room around them exists in pad geometry but not on the grid or after other nets' copper, and four ideas for the grid mismatch (fine local grid, pre-route, bent stubs, wider search) have not moved them. Not tried: routing U1's escape in continuous geometry as one planned unit, with the rest of the board routed around it.
