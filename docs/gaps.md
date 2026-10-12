# Gaps — ranked work list

**Updated:** 2026-10-12. Top item is next. Strike through and date items when
they close; don't delete them.

1. ~~**M0.1 — run the round-trip test on the whole corpus**~~ — closed
   2026-10-12: 116/116 byte-exact.
2. **M0.2 — fuzz the parser** (truncated files, unbalanced parens, huge depth,
   invalid UTF-8, unterminated strings): errors, never panics.
3. ~~**M1.1 — board model, read side**~~ — closed 2026-10-12. Pintype comes
   from the `.net` (boards carry none). Open half: courtyard extent ignores
   arc bulge past an axis extreme; absolute (rotated) courtyard boxes are not
   computed yet — needed by the grader.
4. **M1.2 — board model, write side:** move / rotate / flip a footprint by
   editing only its `(at …)` and layer tokens; check KiCad opens it
   (`kicad-cli pcb drc` runs without a load error).
5. **M2 — grader**, starting with the metrics the Portcullis baseline doc already
   measured on m1 so the first comparison is like for like.

## Known unknowns

- Whether KiCad 9 and KiCad 10 files differ in ways the model must handle
  (the corpus is all KiCad 9). Unverified.
- Flipping a footprint to the back mirrors pads, graphics and text; the exact
  transform must come from the file-format docs, not from guesswork.
