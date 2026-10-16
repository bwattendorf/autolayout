# Gaps — ranked work list

**Updated:** 2026-10-13. Top item is next. Strike through and date items when
they close; don't delete them.

## NOW: M7 — our own router (owner's choice, 2026-10-13)

Why now, and why Rust: Freerouting (GPL) can be run but never shipped inside
autolayout; it sees only nets, not our groups; and on m1 it plateaus (90
unrouted from pass 8 to 10). Portcullis's in-house Python routers plateaued
too; speed per experiment was part of why. Each step below commits on its
own and ends with something measured.

- ~~**M7.0 — design rules from the project.**~~ — closed 2026-10-13 (`autolayout rules`, `crates/route/src/rules.rs`; below).
  Original scope: Read the netclasses (track width,
  clearance, via diameter and drill) from `<board>.kicad_pro` (JSON) beside the
  board. The Freerouting harness silently routed at KiCad defaults (0.2 mm,
  0.6/0.3 vias) because the prepared board was saved without its project.
  m1's real fab rules are 3.5 mil with 0.25/0.15 vias, and Portcullis
  recorded that "the via change is what moved it" to 0 unrouted. A router
  must never guess its rules: if no project is found, refuse, or take rules
  on the command line.
- ~~**M7.1 — routing model.**~~ — closed 2026-10-13 (`crates/route/src/model.rs`, `autolayout model`; see ROADMAP). Original scope: A layer grid at a resolution derived from the
  rules. Obstacles: pads (real rotated shapes, clearance-inflated),
  keepout zones, the board edge, through-hole pads on every layer. Plane
  nets (poured) connect by a via to their plane, not by traces.
- ~~**M7.2 — one net, A\*.**~~ — closed 2026-10-13 (`route::search`, `route::driver`, `autolayout route`; see ROADMAP). Original scope: Deterministic maze search on the 3-D grid (x, y,
  layer) with a via cost and a direction preference per layer. Multi-pin
  nets grow a Steiner-ish tree from the nearest pin pair outward.
- ~~**M7.3 — negotiated congestion.**~~ — closed 2026-10-13 (`route::negotiate`, `autolayout route`; see ROADMAP). Original scope: PathFinder (McMurchie & Ebeling,
  FPGA '95): route every net, let them share cells at a cost, raise the
  history cost of over-used cells, rip up and reroute until no cell is
  shared. Published, well understood, and suited to fixed-grid routing.
- **M7.4 — write and judge.** *(2026-10-13: writer built and read back; KiCad verdict PENDING — run `tools/m7_check.py` on the Windows machine, then close this item.)* Tracks and vias go into the `.kicad_pcb` through
  the lossless layer, into a NEW file. **KiCad DRC is the judge:** 0
  clearance violations, and the unconnected count reported every time.
- ~~**M7.4b — verifier reads zone fills**~~ *(closed 2026-10-13: rounds 4-6, counts equal and 4 of 17 findings matched part by part; still open: via_dangling, exact overlap, other boards)*: KiCad found 17
  unconnected items on Freerouting's m1 that `route::verify` missed because
  it treats a declared plane as solid. Read `filled_polygon` islands per zone
  (format to be confirmed with `tools/zone_probe.py` on a KiCad-saved board,
  docs/windows-handoff.md "Round 3"), join items to the island they overlap,
  count islands that touch no item of their net. Then re-run the Freerouting
  comparison: the verifier should say 17 (or explain every difference). Also
  `via_dangling` (KiCad 9 on Freerouting, 0 on ours). Until then every
  verifier "unconnected 0" on a board with planes carries its LIMIT line.
- **M7.5 — the bar.** *(2026-10-14 round 20 (KiCad 9.0.1, ab9a6f6): the island repair did NOT engage in the batch pipeline. KiCad totals identical to round 19 (unconnected 141, nets complete 4641, 122 / 5 / 4, r and c4 islands still at (81.2588, 62.6132) and (132.0772, 73.0328)); b_route.log says 'island repair rounds 0'. Cause found from the batch script: `tools/m7_batch.py` pours AND FILLS the planes in KiCad before routing (pour() ends with ZONE_FILLER.Fill), so `autolayout route` receives a board whose planes already hold fills computed on the bare board, which have no pockets; the verifier reads present fills and skips the pour emulation (the local evaluation used stripped boards, which have no fills, hence 141 -> 139 there only). Fix: `route` now judges with `verify_fresh_planes`, which ignores the fills of the declared plane layers and emulates them; unit-tested. Local verifier unchanged (stripped boards: 139). The `verify` command still reads fills, correctly, on boards KiCad has saved. Round 21 reruns the batch. 2026-10-14 plane-island repair: after the route portfolio, any tap the emulated pour leaves cut off (`plane_island`) has its via site forbidden for every net (`RoutingModel::block_via_near`) and the net set is routed again with the same stub settings, up to three rounds, kept only if it leaves fewer unconnected (then fewer islands, then fewer violations), else the sites are unblocked. Local verifier, 37 boards, NOT KiCad: unconnected 141 -> 139; the two boards that changed are r/3p5mil-candidate2 (1 -> 0, its island gone) and c4/3p5mil-candidate3 (2 -> 1: the +3V3 via moved; the remaining open pad is U1.2 on N$41, a routing failure, not a pocket); clearance, shorting, hole-to-hole unchanged, no board worse. Round 20 asks KiCad. Cost: c4/3p5mil-candidate3 takes about 430 s locally (four attempts plus one repair). Limit: the repair only avoids the cut-off site; it does not model whether the new site is itself in a pocket (the next round of the loop re-checks, up to three). 2026-10-14 round 19 (KiCad 9.0.1, includes bf913dc): the stub-corridor sharing is KiCad-verified. KiCad unconnected summed over 37 boards 163 (round 13) -> 141, equal to the local verifier's prediction; nets complete 4626 -> 4641 of 4749; five boards improved and none got worse (c4/candidate2 18 -> 12, emu/candidate2 25 -> 23, m1/candidate6 20 -> 15, m1/tp18-candidate2 20 -> 12, g2i 3 -> 2); new clearance / shorting / hole-to-hole identical to round 13 (122 / 5 / 4 absolute, so no DRC entries from shared stubs); verifier = KiCad on unconnected for 37/37. Cost: batch 51 min, summed route time 1943 s against 1007 s in round 13 (about 1.9x; emu/candidate2 287 s -> 612 s). 2026-10-14 round 18 (KiCad python, base c9660df): the footprint-count gap is mostly stale returns. Every return (31 Aug) is older than its freeplace board (1 Sep), every 'dropped' reference is a diode or LED (status lamps, SS34, BAT54S, SMBJ28CA on m2), none a connector or passive; the dropped set differs by board (m2's 12: D49 SMBJ28CA, D51-D60 lamps, D62 SS34), and lamp references shift between files, so a reference diff overstates the drops. The netlists (13 Oct) are newer still: 2-6 resistors per board are in neither board (R54-R59 c1, R67-R72 c2, R68-R71 c2g, R89-R94 c4, R50-R53 e1, R75-R78 e2, R94-R97 m2, R28/R29 r), which is CLAUDE.md's stale r/m2 note. DNP / exclude_from_board cannot be read from the SKiDL netlists (half-answer). Off-board caps: c2 C29 (49.5, 228.0) and io C25 (49.8, 205.5) are locked in the freeplace export 0.2-0.5 mm outside the outline; our placer keeps locked parts, the service moved them 3.1 mm inward. That is a Portcullis freeplace exporter / freeplace_audit gap, not ours. Consequence: Round 17's within-2 comparison holds on the common footprints only; redo against returns made from the current freeplace boards when they exist. Round 16 pin-by-pin: see the entry below it (about 18 U1 pins Freerouting connects and ours did not), which the stub-sharing change targets. 2026-10-14 stub corridor sharing (router): the NOTERM trace on m1/candidate6 showed most unrouted U1 pins had no landing cell at all, and the cause was the stub corridor itself: a stub claims every grid cell within one pitch of its segment, so stubs of adjacent 0.4 mm-pitch pads (0.4 mm apart, less than two corridors) always overlapped, were counted as conflicts, and negotiation dropped one of each pair. Now a stub that stays at least a pitch from every other net's stub keeps its corridor in a separate stub-only class (`NetRoute::stub_cells`, `Costs::share_stubs`): it conflicts with tracks, not with other such stubs; in hard mode two sharing stubs mark the cell SHARED (no track may use it). Unit test: four 0.2 x 0.8 mm pads at 0.5 mm pitch, 2 of 4 nets complete before, 4 of 4 and verifier-clean now. The portfolio is now four attempts in order (reach 1.5 / 4.5, sharing on, then off), stopping at the first complete and clean, keeping the fewest verifier-unconnected. Local verifier, 37 boards, NOT KiCad: unconnected 163 -> 141 (sharing on only: 152; best of on/off per board: 141), clearance 1, shorting 0, hole 0 unchanged, no board worse; improved: g2i 3 -> 2, c4/candidate2 18 -> 12, emu/candidate2 25 -> 23, m1/candidate6 20 -> 15, m1/tp18-candidate2 20 -> 12. Cost: portfolio time up to about 4x on boards that never come out clean. Round 19 asks KiCad to judge it. 2026-10-14 round 16 (KiCad 9.0.1, base 9d9cff4), Freerouting 10 passes (about 2600 s each, none timed out) on the three weak m1 returns, KiCad unconnected ours / Freerouting: candidate6 20 / 78, locked-candidate5 29 / 84, doublesided-candidate4 15 / 92. Freerouting leaves 4-6x more open overall, so on these boards the MCU ceiling is mostly placement. But the pin-by-pin lists (their own txt files, my set difference on U1 pin names) show it is not only placement: U1 pins unconnected in ours and connected by Freerouting: candidate6 6 (U1.7, 10, 26, 27, 36, 50), locked-candidate5 7 (U1.15, 19, 21, 28, 40, 48, 52), doublesided-candidate4 5 (U1.10, 14, 21, 23, 33); common to ours and Freerouting 9 / 17 / 10. So about 18 U1 pin escapes exist that Freerouting finds and ours does not, with U1.10 and U1.21 missed on two boards each: a concrete router gap to chase locally. One seed, default Freerouting settings. 2026-10-14 round 17 (KiCad 9.0.1, base 9d9cff4), placement by routability on 11 more boards with the returned board's own rules (c1, c2, c2g, cm, e1, e2, el, io, m2, r, t), every service row equal to Round 13: our default placement is within 2 KiCad-unconnected of the commercial return on 11 of 11 (ours 0 on eight boards, 1 on c2 and m2, 4 on c2g; the service 1 on r where ours is 0), KiCad clearance 0 on all 11 where the service has 6 on c2, and shorter track on every board. With Round 15 that is 15 of 15 boards within 2. Open: (a) our footprint counts exceed the service's by 2-12 on seven boards (m2 +12, c2 +4, c2g +4, e2 +4, c1 +2, e1 +2; c4 +8 explained in Round 15 as real parts the return lacks), so those comparisons route a larger board for us; the CLAUDE.md note that the r and m2 returns are stale is one likely cause, not checked board by board; (b) two 10 uF caps are off the outline in our placement (c2 C29, io C25; c2's J90 microSD straddles the edge, probably by design); (c) the only open nets are c2g 4, c2 1, m2 1, all on placements with courtyard overlaps or off-board parts, so placement may be the cause (not tested with Freerouting). 2026-10-14 round 15 step 4, c2/3p5mil-candidate2, KiCad 9.0.1: Freerouting 5 passes, 34 min 4 s, finished normally: KiCad unconnected 22, clearance 15 (also 69 track-width violations and 18 dangling vias; its own count ended at 4 unrouted, 28 violations). Ours (round 13): unconnected 0, clearance 6 (absolute, pre-existing in the service placement), 10.1 s. That is the third board with a Freerouting KiCad number against ours (io 29 vs 0, round 12; m1/emu/g2/t in round 7-8); c2 is closed. Batch tooling note: the c2 batch run never wrote results.csv (cause not found); the routed board was judged with kicad-cli directly, and refilling zones first gave the same counts. 2026-10-14 round 15 (KiCad 9.0.1, 1cff82d), placement judged by routability with our router on both sides and the right rules (the .kicad_pro beside the RETURNED board; Round 14's table used the freeplace one and is void): unconnected ours default / ours nohalo / service: c4 2 / 3 / 2, emu 1 / 2 / 0, g2 1 / 2 / 0; nets complete c4 188 / 187 / 189 of 190, emu 214 / 213 / 215, g2 128 / 127 / 129 of 129. Our default placement is within 0-2 unconnected of the commercial return on all three, with 25-40% less copper (c4 10790 mm vs 18201, emu 7563 vs 13326, g2 3893 vs 4530) and 0 KiCad clearance errors where the service's c4 has 3 clearance + 1 shorting. Halo on beats halo off on every board. Caveats: one run per board; c4 and g2 leave 2 and 1 parts off the board in our placement (not investigated); the service's c4 return lacks 8 real parts (D29, D34-36 BAT54S, D38 SS34, D39-41 LEDs) that the netlist and freeplace board have, so its 189/190 routes a smaller board. 2026-10-14 round 14 (KiCad, cdeb223): m1/locked-candidate5's KiCad-only +3V3 pair is not a plane problem. U1.38 simply has no track or via; the fill covers it. Our router logs it unreached, with 11 more U1 pins of that board (U1.48, 21, 15, 7, 36, 42, 3, 44, 10, plus R41.1, U5.2/3) in the same state: it is the MCU-pin escape ceiling again. Local geometry (our parse of the file, not KiCad): U1.33-43 is a column of 0.4 mm-pitch pads at x = 102.15, with back-side C6.2 only 0.72 mm to the right of pad 38 and the nearest +3V3 vias 2.2-2.4 mm away. No fix yet; the open question is whether the locked bypass caps leave any lane at all. Also: c2's DSN export works (the closed-outline assertion is only a warning, raised by footprint J90, microSD; 4 per export, cause inside J90 not found and dropped) and Freerouting on c2 is simply slow (not yet waited out). Round 14's placement table (c4 service row 119/190 nets, 85 unconnected) disagrees with Round 13 (189/190, 2) for the same board, so it is not comparable and is redone in Round 15. 2026-10-14 round 13, KiCad 9.0.1, commit 2b044bb (includes f93c772 and 8b4cf00): the stub-reach portfolio and the calibrated pockets are KiCad-verified. Unconnected summed over 37 boards 186 (round 11) -> 196 (round 12) -> 163 (round 13), equal to the local verifier's prediction; nets complete 4626 / 4749; boards with 0 unconnected and no new violation 23 of 37; new clearance 1 (m1/candidate4, unchanged), shorting 0, hole-to-hole 0; no board worse than round 11. The four m1 returns that regressed in round 12 are back to their round 11 counts and the five that improved keep it (g2i 6 -> 3, c4/candidate2 25 -> 18, emu/candidate2 28 -> 25, tp18-candidate2 23 -> 20, tp18-candidate3 22 -> 15). Verifier = KiCad on unconnected and new violations on 37/37; on the two plane-island boards the verifier's unconnected equals KiCad's (c4/3p5 2, r/3p5 1) at the vias KiCad reports. Cost: summed route time 695 s (r11) / 1596 s (r12) / 1007 s (r13), single runs under different load. Remaining: the 163 are mostly m1 returns (12-35 each) and the two island vias, whose repair is not written. 2026-10-14 placement, Round 12 step 3 (KiCad, `tools/m7_place_compare.py`, ran unmodified): on m1 our placement and the service's both end with 0 unconnected in KiCad with our router (ours default 3018 mm / 469 vias; ours `--no-halo` 2431 mm / 487 vias; service 3040 mm / 468 vias), so on m1 routability is at parity and `--no-halo` routes shorter. On c4 it is not: unconnected 2 / 3 / 2 (default / no-halo / service), ours leaves 2 parts off the board with 17 courtyard overlaps and needs 10.8 m of copper against 18.2 m on the service's placement (not compared like for like: 307 vs 299 footprints, unexplained). One board each; the m1 parity is the first placement result that favours us. 2026-10-14 plane pockets calibrated on Round 12. The first emulation (box erosion, 0.1 mm raster, generous touch) matched KiCad on c4 and r but flagged five boards KiCad does not (c2, el, emu, g2, r's second via). Causes found by rebuilding each case from the routed file's neighbouring vias: (1) a box erosion cuts diagonal channels KiCad's round min-width pass keeps; (2) a tap sitting in an antipad wall with open copper on the far side is connected. Now: a circular erosion element, 0.05 mm raster, and a tap touches only through a thickest-fill cell within the min-width radius of its copper. Result on the 37 local boards (our verifier, NOT KiCad): exactly two cut-off taps, c4/3p5mil-candidate3 (132.0772, 73.0328) and r/3p5mil-candidate2 (81.2588, 62.6132), the two KiCad reported; nothing else. A cut-off tap is now left out of its plane in the connectivity, so the verifier's unconnected on c4 is 2 (KiCad 2) and on r 1 (KiCad 1). Still open: m1/locked-candidate5's +3V3 pad pair C6.1/U1.38 (KiCad only, not examined); the REPAIR (a cut-off tap needs its pad joined another way) is not written, so the router still places those two vias; every other board's agreement rests on 2 positives, which is thin. Fixed two unit tests on the geometries (squeezed and wall-edge). 2026-10-14 stub reach portfolio. Round 12 (KiCad) showed the 4.5-pitch reach was not a win: unconnected 186 -> 196, nets complete 4604 -> 4636, four m1 returns worse (candidate6 20 -> 34, doublesided-candidate4 15 -> 26, locked-candidate5 29 -> 35, candidate4 12 -> 14), five boards better (g2i 6 -> 3, c4/candidate2 25 -> 18, emu/candidate2 28 -> 25, tp18-candidate3 22 -> 15, tp18-candidate2 23 -> 20). The local verifier reproduces KiCad's per-board counts exactly on those nine boards. Sweep of a single global reach on the 37 local boards (verifier unconnected, NOT KiCad): 1.5 -> 184, 2.25 -> 207, 3.0 -> 199, 4.5 -> 194; non-monotonic, so no single value is right. `route` now tries 1.5 and, only if that is not complete and clean, 4.5, and keeps the one with fewer verifier-unconnected (ties to the narrow): 163 on the same 37 boards (clearance 1, shorting 0, hole 0), about 1.4x the time on the boards that need the second try (c4: 140 s). Verifier numbers, UNVERIFIED by KiCad until Round 13. 2026-10-13 per-class widths: across the 37 local boards' projects (four distinct class sets; measured by reading their `.kicad_pro` files), only g2i's Power class differs from Default (0.5 vs 0.25 mm); every other board's Power class equals Default, so routing everything at the Default width is the class width on all but g2i. Not built: a wider net needs the grid to claim a halo for it, and the one board it matters on is already the coarse-rule case. Open until g2i is the target. Also 2026-10-13 `tools/m7_place_compare.py` (never run) routes our placer's output and the service's return of the same board with the same router, for Windows; placement itself (M5) is not started, because the lever list is only halo on/off. Plane pockets: `route::pocket` emulates the pour as a 0.1 mm raster (clearance 0.25, inset 0.5, min thickness 0.2, the harness's numbers) and `verify` now reports taps cut off from the main plane (`plane_islands`, apart from `unconnected`). Measured by our own tool on the 37 local boards, NOT KiCad: it finds the c4/3p5mil-candidate3 island at exactly the via KiCad reported (132.0772, 73.0328; a 0.25 mm via with foreign vias 0.409 mm either side, antipads leave a sliver thinner than the minimum width), 2 on r/3p5mil-candidate2 against KiCad's 1, and 1 each on c2, el, emu, g2 3p5mil where round 11 listed none: those are unexplained until KiCad compares them. Calibration is Round 12 step 2 (the same written files, new verify). Open: whether the emulation false-positives, and the repair (re-tap an island's pad through another via) is not written. 2026-10-13 MCU-pin escape, measured by our file verifier on the 37 local boards, NOT KiCad: widening the stub landing reach from 1.5 to 4.5 pitches moved verifier-unconnected 209 -> 194 (the 1.5 figure predates the shorter stubs, so not a clean A/B). A two-segment dogleg stub along the pad's long axis was built and measured: 196 vs 194, noise, so it was removed. What remains on the weak placements (e.g. m1 candidate6: +1V1, U1 pins) is congestion around the MCU, not pad access; the open half is whether a larger corridor-free fan-out reservation around many-pin parts helps. A 0.5 mm-pitch row of 0.2 mm pads completes only its outer pads (neighbour stub corridors block the middle ones); unit test records this. Round 11, current: 19/37 boards clean in KiCad, 23/37 with no unconnected and no new violation, unconnected 186 (round 8: 307), new clearance 1, shorting 0, hole-to-hole 0; tp18-candidate3 is the one board that got worse, from the MCU-pin escape ceiling. Round 10: on the four service-final boards where Freerouting produced a number our router leaves 0 unconnected against its 12-36, in seconds against minutes; whole corpus 18/37 clean, 22/37 with no unconnected and no new violation, new clearance 1, verifier = KiCad on 37/37 for new violations and unconnected. Open: plane-island blind spot (r, c4/3p5), MCU pin escape on the older candidates, coarse-rule boards (g2i), Freerouting on c2/io, and the per-class track widths. Earlier, round 7: our router beats Freerouting on unconnected on both placements, 0 vs 17 and 6 vs 68; our placement is still worse than the service's for both routers; the clearance-override bug found there is fixed in code but not yet KiCad-judged; net N$4 on our placement is unexplained. Round 8 = all boards + the N$4 diagnosis.)* Original scope: *(2026-10-13: NOT DONE — blocked on three things only the Windows machine has: KiCad DRC, Freerouting, and the `.net` file our `place` needs; see ROADMAP for the model-level numbers and the stall on U1.)* On the same placements and the same rules: m1 ours
  (131 unconnected with Freerouting at default rules), the service's m1
  stripped (65). Beat Freerouting on both, then go after the service.

- **M7.5 route-portfolio speed (2026-10-15, branch m7-router-fix).** The three attempts after the first 
  now run in parallel threads (`std::thread::scope`) and are folded in the same fixed order with the same early stop, so the chosen attempt is the same. 
  Measured with `tools/m7_batch.py` on c4/3p5mil-candidate3: `route_s` 1029.9 -> 271.7 s; r/3p5mil-candidate2 5.6 -> 3.0 s; route and verifier lines identical 
  (c4 189/190, 18201.0 mm, 829 vias, repair rounds 1, unconnected 1; r 88/88, 2948.9 mm, 457 vias, repair rounds 1, unconnected 0). 
  Cost: up to three attempts run that the sequential loop would have skipped, and peak memory is up to four routes' worth. 
  Closed (2026-10-15): `corpus_model.rs::dropped_nets_are_repaired_where_possible` was red since bf913dc (stub sharing on by default): on m1/candidate4 the first pass now completes 89 of 99 (2 dropped, 0 repaired, measured) so there is nothing left for the hard re-route, and the test's premise was stale. The router did not regress. The test now keeps the 89-net floor on the default settings and exercises the repair with sharing off (87 of 99, 5 dropped, 1 repaired, measured).

- **Portfolio attempts are logged (2026-10-15).** `autolayout route` now prints one `portfolio attempt N:` line per attempt (stub reach, sharing, nets, verifier unconnected, violations, clean or NOT clean), and one `island repair round N: kept/rejected` line per repair. Output unchanged on r, c4/3p5mil-candidate3, m1/tp18-candidate3 and t (every results.csv column identical to out/m7r22_all, measured). Answer it gave: c4/3p5mil-candidate3 runs all four attempts and none is clean (each leaves verifier unconnected 2; the repair gets it to 1); m1/tp18-candidate3 runs all four with 16/18/22/15 verifier unconnected in the batch pipeline (out/m7r23_targets), none clean.

- **Many-pin IC escape, first measurement (2026-10-15, branch m7-escape).** On m1/candidate4 all 11 unreached U1 pads (QFN-60, 0.4 mm pitch) sit mid-way between grid lines (phase 0.36-0.74 of the 0.3098 mm pitch); shifting the grid origin swings unconnected 14-33 (measured). The cause is the grid/pad pitch mismatch. See docs/m7-results/2026-10-15-escape-notes.md. No router change yet. 2026-10-16: tried and rejected on m1/candidate4 (KiCad, one run each): a third signal layer (13 and 17 vs 10), wider dog-bone offsets (no whole-edge fan-out found), a 0.4 mm grid lined up with U1 (whole board 44; U1 itself leaves 1 unreached pad), and pre-routing U1 on that grid then the rest on the normal grid (18-132 vs 10, five variants). Open: keep only the escape part of each net and continue from it, which needs the router to start a net from existing same-net copper. See notes Updates 6-7. 2026-10-16 tap repair (Round 24, KiCad): the single-pad failures were plane-tap vias closing a pad's only exit lane; moving them closes fab/emu, cinfront, loop-locked and c4/3p5mil-candidate3 (139 -> 125 unconnected, no board worse). Open: g2i (2 pads, coarse rules) and the MCU-escape boards.

Later and deliberately not first: escape planning for many-pin parts (U1
is the largest failure on every m1 route so far), length and impedance
rules, differential pairs, and the RF checks of M8.

1. ~~**M0.1 — run the round-trip test on the whole corpus**~~ — closed
   2026-10-12: 116/116 byte-exact.
2. ~~**M0.2 — fuzz the parser**~~ — closed 2026-10-12: 20 000 cases, 0
   panics. Invalid UTF-8 is rejected one level up (`read_to_string` errors
   before the parser sees it); the parser itself takes `&str`.
3. ~~**M1.1 — board model, read side**~~ — closed 2026-10-12. Pintype comes
   from the `.net` (boards carry none). Open half: courtyard extent ignores
   arc bulge past an axis extreme; absolute (rotated) courtyard boxes are not
   computed yet — needed by the grader.
4. ~~**M1.2 — move / rotate**~~ — closed 2026-10-12, KiCad agrees on 523/523
   pads after edits; cross-check in `ci.sh`.
5. ~~**M2.1 — grader: overlaps, off-board, wirelength**~~ — closed
   2026-10-12; overlaps match KiCad DRC on 35/35 boards. Item 3's open half
   (arc bulge, absolute courtyards) closed with it.
6. **M2.2 — grader: circuit-aware metrics** (next): switch-loop size, crystal
   trace length, bypass-cap distance to the pin it serves. These need the
   grouping of M3 (which converter, which crystal, which cap serves which
   pin) found from pintype and connectivity, never names — so do M3's
   grouping first and let M2.2 consume it.
7. **M3 — grouping.** M3.1 + M3.2 done 2026-10-12: one role per footprint
   (fixed / MCU / converter / crystal / bypass / field cluster / MCU
   support / residue). Crystals by circuit shape (m1 Y1; r Y1 *and* the
   PN5180's own Y2). Bypass caps on a shared rail are reported with their
   candidate ICs, not guessed. Open:
   - USB pair: needs the connector's D+/D- pins without matching pin
     numbers (rule 2); USB-C pin numbering is standard, so a per-part
     record is the honest route.
   - emu leaves 41 parts in residue, mostly LED resistors behind two driver
     ICs that growth does not pass through. Candidate rule: a residue part
     whose only signal neighbour is one group joins it ("satellite").
   - g2n's ESP32-C6 module (5 bidirectional pins) is not found as an MCU.
   Reference: Portcullis `place_floorplan.py` step 1
   (worktree branch `worktree-agent-abcb674faad74edba`) and its findings:
   POWER-IN (not POWER-OUT) marks supply nets; MCU = ≥ 16 bidirectional
   pins; field clusters seed from fixed through-hole parts. Read it for the
   rules; do not copy its code (AGPL).
7a. **M4.2 — spread (next).** Measured 2026-10-12: m1 v1 leaves 135
   unconnected vs the service's placement's 65 (ROADMAP). Target: beat 65 on
   the same harness. Levers, in order of evidence:
   - *Escape halo* around many-pin ICs. Portcullis measured it on a small
     board: HPWL alone went 3 → 4 unconnected, adding the halo went to 0
     (`controller/docs/placement-comparison-2026-08-20.md`, addendum). Read
     for the idea, not the code (AGPL).
   - *Density limit* in global placement (bin utilisation target).
   - *Field parts toward their own connector's edge.*
   - *Rotation* of two-pad parts to align with their nets.
   Quick proxy per change: Freerouting fan-out escape % (≈ 2.5 min;
   59.7 % ours vs 89.9 % the service's). Confirm with a full 10-pass route.
8. **M8 — RF checks**, two halves. Every threshold comes from a cited
   datasheet layout section or app note (CLAUDE.md rule 7). None is invented
   here: until a number is cited, the check reports a measurement and says
   "no limit sourced".
   - **Intended RF: does the radio circuit work as designed?**
     - *Antenna keepouts:* no copper, parts or pour on any layer in a
       module's antenna area. Corpus case: g2n's ESP32-C6 module (Espressif
       hardware design guidelines give the keepout).
     - *Matching networks:* the match sits between the RF pin and the
       antenna feed, in signal order, as short as the app note asks. Corpus
       cases: r's PN5180 13.56 MHz antenna path and EM4095 125 kHz LF path.
     - *Feed lines:* RF traces kept short with no vias and no stubs, and
       controlled impedance where the stackup allows it. Ground stitching
       along feeds and around module edges. Corpus case: g2c's cellular
       modem.
   - **Unintended RF: what will radiate or pick up?**
     - *Switch-node loops:* hot-loop area (input cap, switch, return) and
       switch-node copper area. M2.2 already measures loop size; this adds
       area and the "no pour under LX" rule. That rule is already a hard
       constraint in Portcullis, from RP2350 datasheet §6.3.8.
     - *Crystals and oscillators:* trace length, guard ground, nothing
       routed under them, distance from board edges and from connectors.
     - *Fast-edge nets:* clocks, SPI/QSPI, USB. Found from the MCU pin they
       come from, not their names. Checked for length, edge proximity, and
       crossing a split or gap in the reference plane: a return-path break
       turns a trace into a slot antenna.
     - *Cable-connected nets:* every net that reaches a field connector
       leaves the box on a cable, the classic unintended antenna. Check it
       is filtered or clamped at the connector, not mid-board. This overlaps
       Portcullis's `fieldprotect_check`, which covers ESD/surge; here the
       concern is emission and immunity.
     - *Plane integrity:* slots, splits and via-fence gaps under fast nets.
   - **How parts are found (rule 2):** by part and structure. A module's
     antenna area comes from its footprint's own keepout zone or documented
     geometry. RF pins come from pintype plus the part's datasheet pin map,
     recorded per part, never matched on net names like `ANT` or `RF_IN`.
   - **What this cannot do:** predict emissions in dB or pass a lab test.
     These are layout-rule checks that catch known causes before fab. A
     field solver or a pre-compliance scan stays the real answer, and the
     report says so.
   - **Order:** after M3 grouping and M2.2, which find the converters,
     crystals and connectors these checks need.
9. **Speed:** the overlap check is O(n²) pairs with a box pre-filter; fine at
   200 parts, revisit with a grid if a placer calls it in a loop.
10. **M1.3 — flip to the other side. NOW NEXT** (2026-10-12): front-only
   assembly is the user's policy, and the service's better-routing m1 moved
   every movable back-side cap to the front. Learn KiCad's exact flip
   transform from KiCad itself (flip a part via pcbnew, diff the file), then
   verify autolayout's flip the way move/rotate were verified (KiCad
   `kicad_pads.py` agreement after edits). Then the placer brings movable
   back-side parts (not test points) to the front, and the escape proxy says
   whether that closes the gap. Converter loops stay one-sided (RP2350
   §6.3.8), which front-only satisfies automatically.

## Known unknowns

- Whether KiCad 9 and KiCad 10 files differ in ways the model must handle
  (the corpus is all KiCad 9). Unverified.
- Flipping a footprint to the back mirrors pads, graphics and text; the exact
  transform must come from the file-format docs, not from guesswork.
