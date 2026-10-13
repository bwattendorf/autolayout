# Roadmap

**Stage:** M2 — grader running and KiCad-verified for overlaps; M3 grouping next. Nothing places or routes yet.
**Updated:** 2026-10-12

## Headline numbers

| metric                                     | value       | measured / estimated                                         |
|--------------------------------------------|-------------|--------------------------------------------------------------|
| corpus boards round-tripping byte-exact    | 116 / 116   | measured 2026-10-12, `ci.sh` with `AUTOLAYOUT_CORPUS` set    |
| rotated-front / back pads meeting copper   | 96.6% / 100% | measured 2026-10-12, m1 fab board, `board` corpus test      |
| pads agreeing with KiCad after an edit     | 523 / 523   | measured 2026-10-12, KiCad cross-check in `ci.sh`, m1        |
| boards where overlaps match KiCad DRC      | 35 / 35     | measured 2026-10-12, `./ci.sh --kicad-full`                  |
| m1 signal HPWL: service's return (the bar) | 3 016 mm    | measured 2026-10-12, `autolayout grade`                      |
| boards placed legally (KiCad DRC clean)    | 1 (m1)      | measured 2026-10-12, `autolayout place` + `kicad-cli pcb drc` |
| m1 unconnected after 10 Freerouting passes | ours 135, service 65 | measured 2026-10-12, `tools/route_check.py` + KiCad DRC |
| boards routed (KiCad-verified)             | 0           | —                                                            |
| m1 (service placement) routed by own router | 104/104 nets in our model; **KiCad DRC PENDING** | measured 2026-10-13, `autolayout route`, self-check only; unverified |

## Milestones

| id | milestone | exit condition |
|----|-----------|----------------|
| M0 | Lossless file layer | every corpus `.kicad_pcb` parses and writes back byte-identical; malformed input returns an error, never a panic |
| M1 | Board model | footprints, pads (position, layer, net, pintype), nets, outline, keepouts readable; footprint move/rotate/flip written back and accepted by `kicad-cli pcb drc` with no new parse errors |
| M2 | Grader | `autolayout grade` reports, from the file alone: courtyard overlaps, parts off-board, ratsnest length, per-net HPWL, bypass-cap distance to the pin it serves, switch-loop size; cross-checked against KiCad DRC on the same file |
| M3 | Grouping | every footprint in exactly one group (fixed / converter / bypass / crystal+USB / per-connector field cluster / MCU support / residue), derived from connectivity and pintype only |
| M4 | Legal placer | rule-based initial placement + legalisation: no courtyard overlap, all on board, fixed parts untouched; beats the staging-grid input on every M2 metric |
| M5 | Optimising placer | wirelength/congestion optimisation (analytical or annealing, seeded); compared against the commercial return on the corpus |
| M6 | Routability | export DSN, route with Freerouting as an external process, report unconnected; placement scored by how well a router finishes it |
| M7 | Own router | M7.0 rules → M7.1 grid model → M7.2 A* → M7.3 negotiated congestion → M7.4 write + KiCad DRC → M7.5 beat Freerouting on m1; steps in `docs/gaps.md`. Router results are unverified until KiCad DRC runs on the Windows machine |
| M8 | RF checks | intended RF (antenna keepouts, matching-network order and length, feed lines, stitching) and unintended RF (switch loops, crystals, fast-edge nets, cable-connected nets, plane gaps) reported on every corpus board, each limit cited to a datasheet or app note; the placer gets them as constraints |

## Progress log

- 2026-10-12 — repository created; CLAUDE.md, licences, M0 started.
- 2026-10-12 — M0.1: lossless parser/writer. 116 of 116 Portcullis board files
  round-trip byte-exact (largest 4.9 MB, 0.45 s release build incl. process
  start). `autolayout info` on `quilter/fab/m1/m1.kicad_pcb` counts 183
  footprints / 3112 segments / 321 vias, matching the counts Portcullis
  measured through pcbnew (`m1-layout-experiment-2026-10-10.md`).
- 2026-10-12 — M1.1: board model read side (footprints, pads, nets, outline,
  courtyard) and netlist reader. Pad transform verified against routed copper:
  on `quilter/fab/m1` 172/178 rotated front pads and 44/44 back pads meet a
  track end or via within 1 µm; the clockwise-rotation alternative scores
  4/178 and the extra-mirror alternative 18/44 back, so the convention is
  measured, not assumed (emu: 357/365, 30/30). Netlist join on m1 freeplace:
  0 netted pads without a pintype. Finding: no corpus board carries `pintype`
  itself.
- 2026-10-12 — M0.2: parser fuzzing. 20 000 seeded cases (noise, truncation,
  substitution, splicing): 0 panics, all 4 833 accepted inputs byte-exact; a
  1 000 000-deep nest is an error, not a stack overflow. **M0 closed.**
- 2026-10-12 — M1.2 (move + rotate; flip still open): `autolayout move`.
  Measured first: 7072/7072 rotated chip pads across 116 files store their
  angle in board terms, so a rotation adds its delta to every pad/text angle.
  Graded by KiCad: after moving R8 to 135°, back-side C4 to 30° and U1 to 45°
  on m1's freeplace board, `tools/kicad_pads.py` (pcbnew) and `autolayout pads`
  agree on 523 of 523 pads; the file diff is 92 lines, every one an `(at …)`;
  `kicad-cli pcb drc` loads it. The cross-check now runs in `ci.sh`.
- 2026-10-12 — M2.1: grader (`autolayout grade`): courtyard overlaps, parts
  off board, half-perimeter and MST-ratsnest wirelength split into signal and
  plane nets (> 16 pads, structural). Courtyard overlaps agree **exactly**
  with `kicad-cli pcb drc` on **35 of 35** placed corpus boards (101 pairs,
  0 false alarms, 0 misses; 20 min, `./ci.sh --kicad-full`). Getting there
  took three tries at one false positive, m1 R6/C22: boxes and convex hulls
  both fill the notch of C22's cross-shaped (concave) courtyard; real
  outlines fixed it but then missed equal-width ganged terminal blocks
  (collinear sides, corners on boundaries) until edge midpoints were probed.
  Both shapes are unit tests. First like-for-like numbers on m1: staging
  input 15 741 mm signal HPWL / 121 parts off board; the service's return
  3 016 mm / 0 off board / 2 overlaps (the known ganged blocks).
- 2026-10-12 — M3.1: grouping, first part (`autolayout groups <pcb> <net>`).
  Part kinds from the part's own value units; supply nets from POWER-IN;
  ground = widest supply; MCU = ≥ 16 bidirectional pins; switching
  converters found from their inductor (switch node = the end on a non-rail
  net that reaches an IC; rail = supply or bulk-to-ground). On m1 all three
  converters match Portcullis's step-1 answer key exactly for inductor, boot
  cap and feedback divider, including U4, whose regulator is a second,
  locally re-declared copy of the AP63201 with SW/BST typed PASSIVE — a
  pintype-only rule misses it. r's fixed 5 V buck correctly has no divider;
  emu correctly has one converter (its other regulator is a linear LDO).
  Known over-reach: bulk caps shared by two converters on one input rail
  (m1 U3/U4 on +12V) are listed for both; separating them needs position,
  which is placement's job.
- 2026-10-12 — M3.2: one role per footprint; crystals by circuit shape.
- 2026-10-12 — M4.1: **first legal placer** (`autolayout place`). m1 from
  the staging board, 2.4 s, deterministic: 121/121 parts placed, 0 off
  board, courtyard overlaps only the 2 pre-existing locked terminal pairs.
  **KiCad DRC on the written file: 0 clearance violations** from placement
  (the first version had 1 — J11, a through-hole header, dropped onto C14
  on the back; through-hole parts now occupy both sides). Signal HPWL
  2 374 mm vs the service's 3 016 mm — **but that is not a win.** The top
  render shows everything movable packed into one blob between J4 and the
  relays, with whole regions empty; the service spreads parts with room to
  route. Short wires on an over-packed board is the classic analytic-
  placement artefact. Routability is unmeasured, so "better" is not claimed.
- 2026-10-12 — M6.1: routability harness. `autolayout strip` removes all
  copper (pcbnew's 9.0 SWIG binding breaks every later object after
  `board.Remove()`, so stripping is done in the lossless file layer), then
  `tools/route_check.py` pours identical planes (structural GND/+3V3 from
  `autolayout groups`), exports DSN, runs Freerouting 2.3.0 externally,
  imports, and lets **KiCad's DRC** count unconnected. Smoke test, our m1 v1,
  2 passes: 278 → 152 unconnected, 975 s (fan-out on the packed placement
  is slow). Router choice recorded: Freerouting is the fixed yardstick while
  placement improves; an own router (M7) comes later and is measured against
  it — Freerouting is GPL, so it can be run but never shipped inside
  autolayout.
- 2026-10-12 — **M6.2: first routability comparison, m1.** Both placements
  stripped to bare pads, identical planes (GND F/In1/B, +3V3 In2),
  Freerouting 2.3.0 `-mp 10 -mt 1`, verdict from `kicad-cli pcb drc`:

  | | ours (M4.1) | service's placement |
  |---|---|---|
  | unconnected before → after (KiCad) | 278 → **135** | 278 → **65** |
  | Freerouting's own count, pass 10 | 101 | 51 |
  | fan-out pins escaped | 259/434 (59.7 %) | 390/434 (89.9 %) |
  | clearance violations | 0 | 14 |
  | copper | 2 075 mm, 120 vias | 2 695 mm, 160 vias |
  | route time | 4 413 s | 4 446 s |

  **The service's placement routes about twice as well.** The 21 % HPWL
  "advantage" of M4.1 was the over-packing artefact the render suggested;
  the router confirms it. Notes: Freerouting leaves 65 on a placement the
  service's own router finished, so this ranks placements and does not
  predict a strong router's result; KiCad counts more unconnected than
  Freerouting reports on both boards, which is why KiCad gives the verdict;
  the fan-out escape % (≈ 2.5 min) separated the two as clearly as the
  75-minute run, so it is the quick proxy for tuning, with full routes to
  confirm. Freerouting's GUI threw an NPE in its plane-drawing code on the
  service's board (AWT thread, routing unaffected); the harness now runs it
  headless.
- 2026-10-12 — M4.2 first levers, measured with the fan-out escape proxy
  (Freerouting, headless, ~1.5–2 min), m1:

  | placement | signal HPWL | fan-out escaped |
  |---|---|---|
  | M4.1 packed | 2 374 mm | 59.7 % |
  | + escape halo (0.65 mm × escapes / 4, clamp 0.15–2.0) | 3 069 mm | **64.3 %** |
  | + halo + spreading (ramped repulsion) | 2 907 mm | 61.8 % |
  | the service's placement | 3 016 mm | **89.9 %** |

  Halo kept (default on); spreading kept as an option, default OFF since it
  measured worse. Neither closes the gap. The service's board differs
  structurally: it moved all six movable back-side caps (C21, C31, C32,
  C36–C38) to the front, while our placer keeps sides. Next: flip.
- 2026-10-12 — **Policy: front-side assembly only** (user, 2026-10-12): parts
  on the back cost a second assembly pass. autolayout's placer will put every
  part it places on the front. Test points stay on the back (bare pads,
  probed from below, nothing to assemble). On m1 that leaves 13 locked MCU
  bypass caps as the only assembled back-side parts — Portcullis's
  `BACKSIDE_BYPASS`, family-wide since 2026-08-31; changing that is a
  Portcullis decision, not taken here.
- 2026-10-12 — **M1.3: flip, verified against KiCad on a whole board.**
  Rules read off files KiCad itself wrote (`tools/kicad_flip_probe.py`, and
  all 183 m1 parts flipped by pcbnew): F.x↔B.x layers; every local y negated
  (pads, graphics, texts, polygons, arcs; 3D model untouched); footprint
  angle θ→180−θ; pads keep board-frame angles; text angle → upright(180−t)
  with `(justify mirror)` toggled (fits all 7 angle classes of 366 texts).
  Every m1 part flipped by autolayout and by KiCad: **523/523 pads identical**
  read by KiCad and read by autolayout; runs in `ci.sh`. Two reader fixes
  fell out of it: board coordinates resolve to whole nanometres (KiCad's
  unit — 10 pads printed 0.1 µm off on exact half-values) and 90° rotations
  are exact. New guard: `tools/compare_pads.sh` refuses empty listings,
  after a comparison reported "0 differing rows" between two empty files.
  Known cosmetic gap: `move`'s rotate adds the delta to text angles without
  KiCad's keep-upright step.
- 2026-10-13 — M4.3: **front-only placement** (default; `--no-halo` keeps
  M4.1 behaviour). Movable back-side parts are flipped with the verified
  flip; the placer reasons on their mirrored geometry. m1: C21 C31 C32 C36
  C37 C38 flipped; the back now holds exactly what the service's board
  holds (13 locked MCU bypass caps, 18 test points); KiCad DRC: 0 clearance,
  only the 2 known locked overlaps. **Escape did not improve: 60.6 %**
  (halo alone 64.3 %, service 89.9 %). So back-side caps were not the gap;
  with sides now identical, the difference is where parts sit. Next levers:
  field parts to their own terminals (the service spreads to the edges and
  beside the connectors; ours still gathers toward the MCU), and rotation.
- 2026-10-13 — **Diagnosis of the routability gap (m1), first round.**
  Facts established:
  - *The comparison was fair:* both DSNs carry identical rules (0.2 mm
    track, 0.2 mm clearance, 0.6/0.3 mm via), and every placement started
    fan-out from the identical state (338 of 452 SMD pins needing fan-out,
    96 already connected through the pours).
  - *Where the router failed* (KiCad DRC on the two 10-pass routes): the MCU
    U1 dominates both, ~56 of ours and ~48 of the service's 65 unconnected
    endpoints. U1 is locked identically on both; on those runs its 13 bypass
    caps sat on the back beneath it — the ring Portcullis measured costing
    escape. Portcullis turned that off today (owner's decision), so this
    share is addressed outside the placer.
  - *Our placer's own deficit:* outside U1, ~60 more unconnected than the
    service's, mostly plane-net pins (GND 38, +3V3 29 vs 14 and 7) — pins
    needing only a stub and a via.
  - *Rejected hypotheses:* local via space per pad (99.8–100 % on every
    board) and via-to-via competition (`autolayout escape`; ranks the two
    routed boards right, the three halo boards wrong). Recorded as NOT
    VALIDATED in `grade/src/escape.rs`.
  Open: the mechanism behind the plane-pin failures is not yet known.
- 2026-10-13 — **Full route of the front-decoupled m1** (caps on the front,
  placed by pin record; same harness, 10 passes, 2 843 s): **131
  unconnected** (KiCad) vs 135 for M4.1 and 65 for the service's placement.
  U1 endpoints **rose 56 → 64**: the caps now ring U1 at 1.35 mm on the TOP,
  blocking its escape there exactly as the back-side ring did below —
  Portcullis's "the ring cannot both decouple and leave the escape alone"
  holds on either face. GND/+3V3 failures outside U1 unchanged (41/29).
  Conclusions: (1) the cap *arrangement* around the MCU must leave escape
  channels (caps at the power pins / corners, gaps at signal pins, still
  ≤ 3 mm) — next lever, scored by U1's unconnected count; (2) the plane-pin
  deficit outside U1 is a separate open problem. The owner's front-side
  decision is a cost decision and is unaffected; this result is about how
  the placer arranges the caps, not where.
- 2026-10-13 — **Decoupling record** (`place --pairs`): with the cap→pin
  record, 17 of 22 m1 caps within 3 mm of their pin, all on the front
  (without it 2 of 22; back-side ring 13 of 22). Escape unchanged (60.4 %).
- 2026-10-13 — **M7.0: design rules from the project** (`autolayout rules`,
  new `route` crate). Reads classes, patterns, assignments and
  `design_settings.rules.min_*` from the `.kicad_pro` beside the board; a
  missing project or a missing/non-positive field is an error that names it,
  and explicit `--track --clearance --via D/DRILL` is the only alternative.
  Measured on the corpus: 97 of 116 board files have a project beside them
  and all 97 load; 19 have none and are refused. Finding: **the two m1
  projects disagree.** `quilter/fab/m1`: track/clearance 0.0889 mm, via
  0.25/0.15 (the fab numbers). `quilter/freeplace/m1` (our input): Default
  0.25/0.2 with a 0.6/0.3 via, Power 0.5/0.25 with 0.8/0.4. Routing the
  freeplace input honours the latter unless rules are given explicitly; which
  to use for the M7.5 comparison is the owner's call. Unverified: pattern
  semantics (`*`/`?` only, explicit assignment first, first pattern wins) are
  assumed, not checked against KiCad; no corpus project uses assignments.
- 2026-10-13 — **M7.1: routing model** (`autolayout model`, `route::model`).
  One grid per routable copper layer plus one via map, built for one net
  class (pitch = track + clearance); cells hold free / one net / blocked.
  Obstacles: rotated rect pads, capsule circle/oval pads, drilled holes
  (hole clearance, hole-to-hole), existing tracks/arcs/vias, keepout zones
  by their tracks/vias flags, board edge at `min_copper_edge_clearance`;
  plane layers (declared with `--plane LAYER=NET`, a stripped board no
  longer says) are not routed on. Unit tests (synthetic board) caught a real
  bug: keepout points are under `pts`, not `polygon`. Measured, m1, 0.1 s:
  at the **fab rules** (pitch 0.178 mm, 608 x 394 cells) 0 of 345 netted
  pads are unreachable (`cargo test`, corpus); at the **freeplace project's
  rules** (pitch 0.45 mm) **249 of 345 pads contain no grid cell centre**
  (242 of 285 SMD). So pad access cannot depend on on-grid cells: M7.2 must
  connect pads off-grid (stub from the pad centre to the nearest usable
  cell) or use a finer grid. Not modelled (documented in the module):
  non-rectangular outlines (bbox only), custom-pad primitives (anchor
  rectangle; 0 on m1), roundrect corners (square, conservative), zone fills
  (4 on the fab board, counted not blocked), footprint-level keepouts.
  Unverified: nothing here is checked against KiCad DRC yet; that is M7.4.
- 2026-10-13 — **M7.2: A\* per net, greedy baseline** (`route::search`,
  `route::driver`, `autolayout route`). Deterministic A* (integer costs, ties
  on state index) from every cell already in the net's tree to the cheapest
  unconnected pad; four-neighbour moves with a preferred direction per layer,
  through-vias between layers; pads with no grid cell inside get a stub from
  the nearest usable cell to the pad centre; plane-net SMD pads get one via
  each. `route` runs plane taps, then signal nets shortest-first, each
  blocking the rest, no rip-up: the number M7.3 has to beat. **Result, m1
  fab board stripped of copper** (service's placement, its own fab rules
  0.0889 mm / 0.25/0.15 via, GND=In1, +3V3=In2 planes, 2 routable layers,
  `autolayout route`, release build): **101 of 104 nets complete, 489 of 494
  pads connected, 2 936 mm of track, 449 vias, 0.3 s**, identical output on
  two runs. Incomplete: GND (C6.2 C8.2 C11.2), +3V3 (R1.1), N$42 (U1.13).
  **UNVERIFIED:** nothing was written or checked by KiCad (that is M7.4); an
  internal self-check (`route::check`: cells legal for the net, no cell
  shared, every pad joined) reports 0 problems, but it uses the model's own
  geometry and is not a grade. Stubs are not checked against the grid at all.
  Not comparable yet to Freerouting's 65 unconnected (different rules: that
  run used 0.2 mm defaults). The `freeplace` m1 board is the unplaced staging
  grid, so routing it says nothing about routability (3 of 104 nets at fab
  rules); M7.5 needs the `place` output, which needs the `.net` file that is
  not in the cloud clone of Portcullis.
- 2026-10-13 — **M7.3: negotiated congestion** (`route::negotiate`;
  `autolayout route` now uses it, `--greedy` keeps the M7.2 baseline).
  PathFinder (McMurchie & Ebeling, FPGA '95): cells may be shared at a
  present penalty that grows 1.5x per iteration plus a history that grows
  on every still-contested cell; only nets on contested cells are ripped up
  and rerouted; if conflicts remain at the cap the best iteration is kept
  and nets sharing a cell with an earlier kept net are dropped and reported.
  Measured, m1 fab stripped, same setup as M7.2, `autolayout route`:
  greedy **102 of 104** nets (U1.27, U1.28 unreached), negotiated **104 of
  104 nets, 494 of 494 pads, 2 989 mm, 462 vias, 0.5 s**; contested cells
  per iteration 45, 15, 6, 4, 1, 1, 1, 0 (8 iterations); 0 nets dropped;
  identical bytes on two runs. Also changed: plane-tap reach 1.5 -> 5
  pitches (`TAP_REACH_PITCHES`), because 1.5 left 4 of 104 plane pads
  without a via (GND C6.2 C8.2 C11.2, +3V3 R1.1); those stubs reach up to
  ~0.9 mm and are not grid-checked, so they are the first thing to look at
  in the DRC. **UNVERIFIED:** not written, not checked by KiCad (M7.4); the
  self-check (0 illegal cells, 0 shared cells, 0 disconnected pads) is the
  model's own geometry and not a grade. The synthetic unit board only proves
  convergence, determinism and legality, not that negotiation beats greedy;
  the m1 numbers are the evidence, from one board.
- 2026-10-13 — **M7.4: write the routes; KiCad's verdict still owed.**
  `autolayout route ... --out NEW.kicad_pcb` appends `(segment …)` and
  `(via …)` items (KiCad 9 layout copied from the fab board, deterministic
  UUIDs, placed before `embedded_fonts`) through the lossless layer; refuses
  an existing path or the input; reads the file back off disk (parses,
  round-trips byte-exact, copper count equals what was written). m1 fab
  stripped: **840 segments + 462 vias added, 0 original lines changed**
  (`diff` shows 0 removed, 10 416 added), read-back 840/462. Unit test: the
  original items survive in order and untouched, copper sits before
  `embedded_fonts`, same routes give the same bytes. **Not done: the
  verdict.** No KiCad in the cloud, so `kicad-cli pcb drc` has not seen this
  file. `tools/m7_check.py` (pour In1/In2 planes, route, refill, DRC both)
  is written but NOT RUN; expect small first-run fixes. Until it reports,
  every router number above is unverified, in particular the plane-tap stubs
  (up to ~0.9 mm, never grid-checked) and the bounding-box board outline.
- 2026-10-13 — **M7.5 attempted, not closed; one real bug found.** What was
  run: our router on the service's m1 placement (fab board stripped) at the
  rules Freerouting was given in M6.2 (`--track 0.2 --clearance 0.2 --via
  0.6/0.3`; note explicit mode sets the edge clearance to 0.2, the project's
  is 0.3), 2 routable layers + In1 GND / In2 +3V3 planes. Result in our
  model, `autolayout route --iters 60`, 53 s: **73 of 104 nets complete, 391
  of 494 pads, 21 nets dropped for sharing cells; contested cells stall at
  ~104 from iteration 20 on** (334 -> 119 by iteration 11, then flat). The
  incomplete nets are mostly U1's (MCU) pins plus D90/J10: at 0.45 mm pitch
  the 0.5 mm-pitch part's escape is the wall, as in every Freerouting run.
  Same board at its own fab rules: 104 of 104 (M7.3). So the rules, not the
  search, set the result; the decision about which rules the comparison uses
  is still the owner's (open since M7.0). **Not comparable to Freerouting's
  65** (that is KiCad's count of unconnected items after its 10 passes; ours
  is nets complete in our own model, UNVERIFIED). Missing for a real M7.5:
  (1) `tools/m7_check.py` run on the Windows machine for the DRC verdict;
  (2) our own placed m1 (needs the `.net`, absent from the cloud clone of
  Portcullis); (3) the Freerouting 65/131 re-run at the *same* rules.
  **Bug found and fixed:** the present-sharing penalty (x1.5 per iteration)
  overflowed `u32` near iteration 50 and the search ran away until a failed
  8 GB allocation killed the process (exit 134/137). Penalties are capped
  (`PENALTY_CAP`), path costs are `u64`, and a regression test runs 200
  iterations from `u32::MAX` penalties.
- 2026-10-13 — **M7.4 round 2: fixes from KiCad's first verdict** (fab m1,
  KiCad 9.0.1 on commit 8bb50a4: ours 2 unconnected + 2 track_dangling + 2
  hole_to_hole; docs/m7-results/2026-10-13-fab-m1.md). Three causes found,
  all in the router, none in the rules:
  1. *hole_to_hole*: two +3V3 plane-tap vias one pitch (0.1778 mm) apart, holes
     0.0278 mm apart vs 0.0889. Same-net vias were exempt from the via check
     (halo ownership allowed overlap for the same net), and nothing checked
     via-to-via hole spacing at all. Now `RoutingModel::via_spacing` = drill +
     `min_hole_to_hole`; fixed vias mark `hole_block`, a path that places two
     vias too close to each other or to this net's earlier vias bans the
     later site and searches again.
  2. *dangling +12V / +12V_RDR stubs*: not stubs. Both tracks end inside the
     **square** outline of a roundrect pad (C22.1 3.5 x 1.6, C28.1 1 x 1.45,
     ratio 0.25) but outside its rounded corner, 0.068 / 0.069 mm from the
     copper. M7.1 had written "roundrect corners count as square
     (conservative)": true for obstacles, **wrong for landing a track**. Pads
     now have `pad_copper` (true rounded rectangle, `roundrect_rratio`, 0.5 if
     unstated) for access and `pad_obstacle` (full rectangle) for clearance,
     and an access cell must lie half a track-width inside the copper.
  3. *self-check*: the old `check` re-read the router's own grid, so it shared
     every blind spot. Replaced by `route::verify`, which reads the **written
     file** (segments, vias, true pad shapes) and judges unconnected,
     track_dangling, hole_to_hole, clearance, copper_edge, track_width and
     via_size in continuous geometry. `autolayout route` now verifies the
     exact text it writes and exits 1 on any finding; `autolayout verify
     <board>` does it for any file.
  **Agreement with KiCad (measured, one board, calibrated on it):** the
  verifier run on the *old* router's output (built from 739824f) reports
  unconnected 2, track_dangling 2, hole_to_hole 2, clearance 0 at the same
  coordinates KiCad listed, versus KiCad's 2 / 2 / 2 / 0. Getting there took
  four wrong guesses at KiCad's joining rules, each shown by the board: a
  track end joins a pad when its copper reaches the pad polygon (calibrated:
  threshold = half the track width + 0.01 mm, since ends up to 0.0528 mm
  outside the true corner were joined and the two at 0.068/0.069 were not); a
  track end on another track's body joins it; a via or pad centre on a
  track's body joins it; a via whose disc reaches a pad joins it. A 0.0001 mm
  clearance shortfall KiCad ignored is also ignored (1 um slack). These are
  fitted to 35 ends on one board, not read from KiCad.
  **Current router, same board and rules:** 104/104 nets, 925 segments, 460
  vias, 3 009 mm, verifier 0/0/0/0, identical bytes on two runs. UNVERIFIED by
  KiCad (round-2 DRC owed). Also: plane-tap and off-grid stubs are now
  checked against foreign pads (at 0.2 mm rules a tap stub crossed the MCU's
  0.4 mm-pitch pads: verifier clearance findings 35 -> 2; that case is 62 of
  104 nets, verifier unconnected 57, clearance 2, not good). Stubs are still
  not checked against other nets' *tracks*.
  **Correction to M7.1:** "0 of 345 pads unreachable at fab rules" used the
  square-corner pad outline. With true rounded copper and a half-track inset,
  21 of 345 pads have no grid cell fully on their copper and land by an
  off-grid stub instead (`m1_fab_every_pad_is_reachable` is replaced by
  `m1_fab_routes_and_the_verifier_is_clean`, which routes, writes and verifies
  the whole board: 104/104 nets, verifier clean, measured 2026-10-13).
- 2026-10-13 — **M7.4 round 2 verdict (KiCad 9.0.1, commit 49bd01a, fab m1,
  `docs/m7-results/2026-10-13-fab-m1-round2.md`).** Our router: KiCad reports
  **0 in every routing category** (round 1: 2 unconnected, 2 track_dangling, 2
  hole_to_hole); 104/104 nets, 925 segments, 460 vias, 3 009 mm, 0.7 s. Only
  the five non-routing kinds already on the stripped board remain. **The
  three fixes worked.** Freerouting on the same board and rules (reproduced
  exactly): 17 unconnected, 2 clearance, 2 shorting_items, 45 track_width, 9
  via_dangling, 6 min 25 s. Verifier vs KiCad on **Freerouting's** board (the
  test it could fail): track_width 45/45; the four clearance-type spots
  agree (KiCad calls two of them shorting_items, the verifier now does too);
  **unconnected: KiCad 17, verifier 0 — a real gap.** Cause: told a layer is
  a plane, the verifier joins every via and pad of that net to it
  unconditionally, so it cannot see a pour split into islands (13 of the 17
  are GND fragments on F/B, 3 are +3V3 track pairs, 1 is a GND pad pair).
  Agreement on our own board never tested this: both said 0. **Open:** read
  zone fills (`filled_polygon`) instead of assuming planes; needs the real
  file format, so `tools/zone_probe.py` + "Round 3" in docs/windows-handoff.md
  ask for it. Done now: `shorting` is its own category; the report prints a
  LIMIT line (e.g. 168 pads/vias assumed on planes for our fab board) so
  "unconnected 0" cannot be read as covering plane islands. Still not
  implemented: via_dangling. Stress case at 0.2 mm (model only): 62/104 nets,
  verifier unconnected 57, clearance 2; not KiCad-judged.
- 2026-10-13 — **M7.4b: the verifier reads zone fills** (round-3 data,
  `docs/m7-results/2026-10-13-zone-data.md`, KiCad 9.0.1). What the files
  show: each `filled_polygon` is one island of copper with its own `(layer)` and
  `(pts)`; a hole is a slit in the same ring (even-odd containment handles it);
  no `(island)` token is ever written (so it cannot be relied on); KiCad's 13
  zone-vs-zone findings on Freerouting's m1 are 13 GND fill polygons (F: 13, B:
  3, In1: 3 in the file) with nothing tying them together. Our board has two
  single-polygon zones. Implementation: every fill polygon is a node of its
  net; a same-net pad, via or track lying on it (sampled points) joins it; a
  track end inside it is not dangling; components = those holding a pad, a
  fill or an assumed plane. A declared plane that has a fill in the file is
  read, not assumed; one without (a stripped board, as in the router's own
  in-process check) still falls back to the assumption and prints LIMIT.
  Unit tests: one fill joins two pads, two fills leave the net split, a lone
  island is a component, a track repairs it, wrong net/layer joins nothing, a
  slit ring excludes its hole. **Not yet checked against KiCad**: the
  acceptance test is `autolayout verify` on the round-2 Freerouting board
  saying about 17 unconnected (14 GND + 3 +3V3) and 0 on ours; pad/via/track
  overlap is by sample points, not exact, and thermal-relief rules are not
  modelled. Round 4 in docs/windows-handoff.md.
- 2026-10-13 — **M7.4b round 4 (KiCad 9.0.1, verifier at b997df0,
  `docs/m7-results/2026-10-13-verify-fills.md`).** The fill-reading verifier
  now agrees with KiCad on Freerouting's m1 in every category it implements:
  unconnected **17 vs 17** (GND 14 vs 14 = 15 components; +3V3 3 vs 3 = 4
  components), clearance 2 / shorting 2 / track_width 45, all with the same
  coordinates and values; ours 0 vs 0; fill-polygon count 21 as in the probe;
  no LIMIT line (fills present). Round 2 had said 0 on that board. **What is
  and is not confirmed:** equal counts on a board it did not help make, on
  two nets, is real evidence; but the verifier's printed locations did not
  match KiCad's, so *which* components it counts is not shown to be the same
  ones. Two reasons, neither a counting error: KiCad prints one point per
  zone, not per island (all 13 GND fragments are at the zone anchor (50.425,
  50.425)), so a fill island has no location to match; and the verifier
  named one arbitrary pad per extra component, capped at 6 per net (hence 9
  lines for 17 findings), with a meaningless "inf mm" distance. Fixed now:
  each extra component is listed with its pads (`REF.PIN`), track count and
  the box around its track ends, vias and fill islands (layer + box), main
  part = most pads, up to 40 listed per net. Round 5 asks Windows to check
  that C2.2 and U1.47 land in different GND parts and that each KiCad +3V3
  track coordinate falls in a listed part's box. Still not implemented:
  via_dangling (KiCad 9 on Freerouting, 0 on ours).
- 2026-10-13 — **M7.4b round 5 (KiCad 9.0.1, verifier at 646d312,
  `docs/m7-results/2026-10-13-verify-parts.md`): parts vs KiCad's items.**
  Confirmed: GND pad pair — U1.47 is alone in GND part 15, at exactly KiCad's
  U1.47 point, and C2.2 is in the main part, so KiCad's "C2.2 needs U1.47"
  is the verifier's "part 15 needs the main part". Counts 14 GND + 3 +3V3 =
  17 as before. **My expectation was wrong:** I predicted 13 fill-only GND
  parts; there are none. All 13 fill islands hold pads (e.g. part 2:
  C36.2 U7.4 D27.1 C26.2 C19.2; part 10: U1.61), so each part is an island
  that holds GND pads but reaches no via or track to the rest. KiCad reports
  those as zone-vs-zone at the zone anchor (50.425, 50.425) with no pad
  named, so a one-to-one match of the 13 is impossible from the DRC report
  alone: 13 = 13 is a count, not an identity. Not resolved: the +3V3 pairs.
  Printed track-end boxes of parts 2 and 3 overlap, so for pair 2 and pair 3
  a box test cannot say whether the two KiCad points sit in different parts
  (pair 1 came out *both in part 2's box*, which a "two different parts"
  reading would not predict; the box is of track ends, not of tracks).
  Added: `autolayout verify --probe X,Y` (repeatable) prints, for each point,
  every pad/track/via/fill within 0.005 mm and which part of its net it is in,
  and duplicate pad numbers in one footprint now get distinct labels
  (`SW2.2`, `SW2.2#2` — the "SW2.2 in two parts" of round 5 was two pads both
  numbered 2). Round 6 asks Windows to probe KiCad's six +3V3 points.
- 2026-10-13 — **M7.4b closed: round 6 probes (KiCad 9.0.1, verifier at
  431e247, `docs/m7-results/2026-10-13-verify-probes.md`).** `verify --probe`
  on KiCad's eight pair points on Freerouting's m1: every probe found the
  item KiCad names, same net and layer; **all four pairs land in two
  different verifier parts** — +3V3 #1 parts 1 and 2, #2 parts 3 and 2, #3
  parts 3 and 4; GND pad pair C2.2 (main part) and U1.47 (part 15). With
  round 4 (counts 17 = 14 + 3, clearance/shorting/track_width equal) this is
  agreement with KiCad on component identity for 4 of 17 findings plus equal
  counts for the other 13. Not matched, by the nature of KiCad's report: the
  13 GND zone-vs-zone items (all printed at the zone anchor (50.425, 50.425),
  where the probe finds no copper). Extra copper found at probe points but
  not named by KiCad (other-layer tracks, other-net pads) is expected on a
  4-layer board and was not judged. One probe quirk: fills are listed by
  XY on any layer, not only the item's. Open for the verifier: `via_dangling`
  (KiCad 9 on Freerouting, 0 on ours), exact (not sampled) pad/fill overlap,
  and agreement on boards other than m1.
- 2026-10-13 — **M7.5 round 7: our own placement, judged by KiCad 9.0.1**
  (`docs/m7-results/2026-10-13-m7-5-placements.md`, commit 1e4fcf9, same fab
  rules everywhere). m1's netlist is `controller.net` (no `m1.net` exists);
  regenerated with Portcullis `run-checks.sh` (exit 0). `place`: 121/121 parts
  placed, signal HPWL 15 741 -> 3 051 mm, only the 2 known locked overlaps.

  | KiCad | unconnected | clearance | shorting | track_width | via_dangling | copper mm | vias |
  |---|---|---|---|---|---|---|---|
  | service placement + our router | **0** | 0 | 0 | 0 | 0 | 3 017 | 460 |
  | service placement + Freerouting | 17 | 2 | 2 | 45 | 9 | 3 306 | 232 |
  | our placement + our router | **6** | **8** | 0 | 0 | 0 | 2 978 | 464 |
  | our placement + Freerouting | 68 | 4 | 0 | 22 | 7 | 3 261 | 231 |

  Findings. (1) **Our router beats Freerouting on unconnected on both
  placements** (0 vs 17, 6 vs 68) at the same rules; time 0.7 s / 16 s against
  377 s / 732 s. (2) **Our placement is still worse than the service's for
  either router** (6 vs 0, 68 vs 17): the placer, not the router, is behind;
  this reproduces M6.2/M4.3's ranking. (3) **A real bug the verifier missed:**
  the 8 clearance violations are VBUS tracks 0.107-0.124 mm from GND pads of
  J10 (USB-C), whose footprint carries `(clearance 0.13)`; the router, its
  stub check and the verifier all ignored footprint/pad clearance overrides, so
  the verifier said 0 where KiCad said 8. Freerouting's 4 are the same J10
  override (worst 0.095). The fab board has no such overrides (it was
  reconciled); the freeplace board, our input, has them (31 at 0.13, 4 at
  0.156776, 2 at 0.05 across 30 corpus files). Fixed: `Pad.clearance` (own
  override, else its footprint's) is read, the model keeps max(rule, override)
  around those pads, tap/off-grid stubs respect it, and the verifier uses
  max(override-or-class of each item, board minimum). NOT yet re-judged by
  KiCad. (4) Open: 103/104 nets complete on our placement, all 6 unconnected
  on net N$4 -- cause unknown, needs the board (Round 8, part B). Housekeeping
  for the owner: running `run-checks.sh` rewrote tracked `*_skidl.log` files in
  Portcullis; they were not reviewed or committed.
  Also found in a local dry run of strip/plan/route/verify over the 37
  service-placed boards (cloud, no KiCad, partial): `finish/g2i` gave 18
  verifier clearance findings because `+12V` is in a 0.25 mm Power class
  while every net is routed under Default (0.2 mm). Now the spacing satisfies
  the largest class clearance (conservative; Power nets still get Default
  track widths, so current capacity is not honoured). Unit-tested, not
  KiCad-judged. The local dry run was stopped after 6 boards to commit; it is
  not a result.
- 2026-10-13 — **M7.5 round 8: our router over all 37 service-placed boards,
  KiCad 9.0.1** (`docs/m7-results/2026-10-13-round8.md`, commit a3cfc79,
  `tools/m7_batch.py` ran unmodified). 37 of 37 ran without error; **16 of 37
  have 0 KiCad routing violations**; nets complete 4 616 / 4 749; KiCad
  totals: unconnected 307, clearance 137, shorting 8, hole_to_hole 4. Clean
  boards are mostly the service's final candidates (`3p5mil-*`, fab m1/emu);
  the worst are the earlier variants (m1 locked-candidate5: 61 unconnected, 74
  of 99 nets; emu candidate2: 41; c4 candidate2: 36; m1 candidate4/6: 34).
  **Verifier vs KiCad:** equal on unconnected for every board except g2i (KiCad
  1, verifier 10), c2 and c4/3p5mil-candidate3 (0 vs 1); KiCad counts MORE
  clearance on 12 boards (up to 19 vs 5), plus shorting/hole_to_hole it does
  not see on c2, c4, r and others. The batch did not record violations that
  exist before routing (pad-to-pad in the service's own placement), so
  whether those excess clearance counts are the router's is **unknown**;
  `tools/drc_compare.py` and the batch's new `*_before` columns exist to
  answer it (Round 9). The reports were not copied into the repo, so none of
  this could be examined from the cloud.
  **Net N$4 (our m1): explained.** Not unreachable pads: the negotiation sat
  on 1 contested cell for ~50 iterations and the final legalisation dropped
  the whole net (all 7 pads). Same mechanism on the candidate boards (verifier
  unconnected = KiCad's, reproduced locally to the net: m1 candidate4 87 of 99).
  Fixed: (1) dropped nets are now re-routed in hard mode around the nets that
  were kept (`repaired_nets`); (2) history growth accelerates while the
  contested count is stuck. Measured locally, verifier unconnected: m1
  candidate4 34 -> 29 (nets 87 -> 89 of 99), g2 candidate6 2 -> 1; m1
  3p5mil-candidate1 unchanged (clean). The rest are MCU pins (U1.xx, 0.2 mm
  pads at 0.4 mm pitch: no cell fits inside the copper, escape cells are
  blocked by the placement) — placement-limited, as in every Freerouting run.
  Not yet KiCad-judged. Local vs Windows: the same boards routed on the
  pcbnew-saved copy differ by 1-2 nets in two cases (m1 3p5mil-candidate1: 98
  of 100 there, 100 of 100 here; emu candidate2 188 vs 187), so pcbnew's
  rewrite of a board changes pad coordinates enough to change a route; both
  are deterministic, neither is "the" answer.
- 2026-10-13 — **M7.5 round 9: where KiCad's extra clearance counts come
  from** (`docs/m7-results/2026-10-13-round9.md`, commit 784e95b, ten
  boards, `tools/drc_compare.py`). Of 107 KiCad clearance entries on the
  ten routed boards, **93 already existed in the service's placement before
  any copper was added** (pad against pad, one connector's pads: J10 on four
  m1 boards, J92, J26, J19/J14, J29/J20) — not the router's, and the verifier
  rightly ignores pad-vs-pad. **14 are new and all involve our copper** (7
  track/via vs track/via, 7 track/via vs pad); none involves a zone. Shorting
  and hole_to_hole on c2 and c4/3p5mil were pre-existing; board `r` has 2 new
  clearance and 2 new shorting entries. So the verifier agreed with KiCad on
  the new violations on 8 of 10 boards and missed some on c2 (1 of 2) and r
  (all 4).
  Causes found in the 14 new entries and fixed (local, unit-tested, **not yet
  KiCad-judged**):
  1. *Custom pads* (board r, U6 pads 15/16: a 0.115 mm anchor plus a 0.845 mm
     polygon): only the anchor was modelled, so a track beside the polygon
     looked clear — KiCad: a short and a 0.0154 mm violation. `Pad.primitives`
     (`gr_poly`, stroke and fill) and `Shape::Union` now model them; other
     primitive kinds are counted in `unmodelled_primitives`, not drawn.
  2. *Micrometre shortfalls* (0.0006-0.0027 mm, round/through-hole pads on c2,
     c4, m1, r): KiCad polygonises curves within the project's `max_error`
     (0.005 mm, all 32 corpus projects). The router now keeps that much extra
     clearance (pitch grows by 0.005 mm), and the verifier's slack is cut from
     1 um to 0.2 um because KiCad does flag 0.6 um. (Round 2 had the opposite
     impression from a 0.1 um case; that was a modelling error since fixed.)
  3. *Stubs against tracks and vias* (0.011-0.030 mm, c4/candidate2,
     emu/candidate2; lengths like 0.3971 mm are off-grid stubs): stubs were
     only checked against pads. A stub now claims its corridor (cells within
     one pitch of the segment) as part of the net, so other nets' tracks and
     via halos conflict with it and negotiation routes around; landing cells
     whose corridor another net already owns are rejected. Cost, measured: the
     MCU pin U1.2 of c4/3p5mil-candidate3 can no longer find a clear landing
     (190 of 190 nets -> 189 of 190).
  4. *g2i, verifier 10 unconnected vs KiCad 1:* nine through-hole GND connector
     pads (pad has its own `(zone_connect 1)`). Hypothesis, not confirmed: a
     thermal relief leaves the pour a gap short of the pad, reached by spokes,
     so no point inside the pad lies in the fill. The verifier now joins a pad
     to a same-net fill that comes within the pad's outer radius + 0.05 mm.
     Whether that is KiCad's rule for those nine pads is for Windows to show.
  5. *Stub length:* stubs now end just inside the pad's edge (half a track
     width + 2 um in) on the line to the centre, not at the centre; shorter
     stubs, smaller corridors.
  Measured locally after 1-3 (verifier = KiCad on these boards before), m1 fab
  104/104 clean; r 88/88 and c2 150/150 with 0 clearance/shorting; c4/3p5mil
  189/190. Local run of 36 of the 37 boards (the local script's glob misses
  `returned/m1-2026-08-06-candidate5`), before the stub-length change, versus
  KiCad's round 8: nets complete 4 497 vs 4 534 on the same boards; verifier
  unconnected 209 vs KiCad 307; verifier clearance 1 (KiCad's new ones were
  14, pad-vs-pad ones are ignored by design). Boards with MORE nets complete:
  g2, m1 3p5mil-candidate1, radialv2, m2, locked-candidate5, routed/m1.
  Boards with FEWER: the older candidate placements (c4/candidate2 168 -> 158,
  emu/candidate2 188 -> 182, m1 candidate4/6/doublesided/tp18 by 2-6) and
  finish/g2i (31 -> 25), the price of keeping stubs clear of other nets and the
  max_error margin. After the stub-length change, spot checks: m1 candidate4
  unconnected 25 -> 12 (87 of 99 nets), g2i 8 -> 6 (26 of 32), m1 fab, r, c2,
  g2 clean. g2i's 31 -> 26 stays a real cost of the corridor on a coarse-rule
  board (0.505 mm pitch, 23 of 88 pads need a stub).
- 2026-10-13 — **M7.5 round 10: KiCad judges the round-9 fixes; Freerouting on
  the service-final boards** (`docs/m7-results/2026-10-13-round10.md`; steps
  1-4 on commit `d0a443e`, **not** the later stub-length change `8d610bc`).
  Whole corpus, our router, KiCad 9.0.1: boards with 0 routing violations
  16 -> **18**; boards with 0 unconnected and 0 *new* violations **22 of 37**;
  KiCad unconnected 307 -> **216**; **new** clearance **1** (was 14 on the ten
  boards of round 9), new shorting **0** (was 2), new hole_to_hole 0; nets
  complete 4 616 -> 4 574 (fewer: unconnected counts fell because hard-mode
  repair now leaves partial nets connected, while nets that cannot land a
  stub clear of others now stay incomplete). The one new clearance is
  m1/candidate4 (14 pre-existing + 1). The verifier now agrees with KiCad on
  unconnected and on every new violation on **37 of 37** boards (and the
  thermal-relief rule removed g2i's nine phantom GND parts; KiCad's own g2i
  count rose 1 -> 8 with the router at 25 of 32 nets). Worse than round 8, all
  small: c4/3p5mil-candidate3 0 -> 2 (U1.2 cannot land a stub), m1-2026-08-06-
  candidate5 0 -> 5, g2i 1 -> 8, r 0 -> 1. **Local check on the current build
  (`8d610bc`):** m1-2026-08-06-candidate5 back to 82/82, g2i 26/32, r 88/88,
  c4/3p5 still 189/190 (net N$41, U1.2). **New KiCad finding, a model gap:** r
  has one unconnected item and one via_dangling, c4/3p5 one via_dangling: a
  +3V3 plane-tap via (and with it pad U1.38 / U1.44) sits on a piece of the
  In2 pour that the neighbouring vias' antipads have cut off. The router's
  in-process verifier assumes planes are solid (LIMIT line) and cannot see
  it; only a verify run on the KiCad-filled board does (it said 1, matching
  KiCad). Plane integrity under via fields is not modelled; open.

  **Freerouting vs ours, same boards, same rules, KiCad unconnected** (10
  passes; single runs):

  | board | ours | Freerouting | ours s | Freerouting s |
  |---|---|---|---|---|
  | returned/m1/3p5mil-candidate1 | 0 | 13 | 1.1 | 282 |
  | returned/emu/3p5mil-candidate3 | 0 | 36 | 9.5 | 2 326 |
  | returned/g2/3p5mil-candidate6 | 0 | 27 | 6.6 | 745 |
  | returned/t/3p5mil-candidate6 | 0 | 12 | 0.4 | 118 |
  | returned/c2/3p5mil-candidate2 | 0 | no number | 13.0 | KiCad assert: non-closed outline, no DSN |
  | returned/io/3p5mil-candidate4 | 0 (197/197, from an earlier run) | no number | 19 | timed out at 3 600 s (log shows done ~70 s later) |

  So on the four boards where both produced a number, **our router left 0
  unconnected where Freerouting left 12-36**, in a thousandth to a hundredth
  of the time. Not compared: clearance/width counts for Freerouting on these
  boards (round 2 on fab m1: 2 clearance, 2 shorting, 45 track_width, 9
  via_dangling), and the older candidate placements, where our router still
  leaves 15-36 unconnected (m1/candidate4 25, emu/candidate2 36, c4/candidate2
  29, locked-candidate5 29): the MCU pins those placements wall in.
- 2026-10-12 — record fix: the M0.1 ROADMAP/gaps updates were made with a
  Python string replace that read the files as cp1252, matched nothing and
  reported nothing. Rewritten with the editor. Lesson: an edit that can miss
  silently must be checked by reading the result back.
