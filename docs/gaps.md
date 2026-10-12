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
5. **M2 — grader** (next), starting with the metrics the Portcullis baseline
   doc measured on m1 (switch-loop size, crystal trace, bypass distance,
   courtyard overlap, parts off-board, ratsnest length) so the first
   comparison against the service's return is like for like. Needs absolute
   courtyard boxes (item 3's open half).
6. **M1.3 — flip to the other side.** Deferred, not dropped: the first placer
   keeps every part on the side it starts on. That is also what the RP2350
   layout rule wants for converter loops (input cap, inductor, output cap on
   one side), so it costs nothing yet. Needed before the placer may choose
   sides.

## Known unknowns

- Whether KiCad 9 and KiCad 10 files differ in ways the model must handle
  (the corpus is all KiCad 9). Unverified.
- Flipping a footprint to the back mirrors pads, graphics and text; the exact
  transform must come from the file-format docs, not from guesswork.
