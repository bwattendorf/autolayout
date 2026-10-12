# Gaps — ranked work list

**Updated:** 2026-10-12. Top item is next. Strike through and date items when
they close; don't delete them.

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
