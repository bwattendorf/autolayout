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
- **M7.5 — the bar.** *(2026-10-13: NOT DONE — blocked on three things only the Windows machine has: KiCad DRC, Freerouting, and the `.net` file our `place` needs; see ROADMAP for the model-level numbers and the stall on U1.)* On the same placements and the same rules: m1 ours
  (131 unconnected with Freerouting at default rules), the service's m1
  stripped (65). Beat Freerouting on both, then go after the service.

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
