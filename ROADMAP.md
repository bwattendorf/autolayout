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
| boards routed                              | 0           | —                                                            |

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
- 2026-10-12 — record fix: the M0.1 ROADMAP/gaps updates were made with a
  Python string replace that read the files as cp1252, matched nothing and
  reported nothing. Rewritten with the editor. Lesson: an edit that can miss
  silently must be checked by reading the result back.
