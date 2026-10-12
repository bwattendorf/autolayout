# autolayout — instructions for agents

autolayout is an open-source placement and routing engine for KiCad boards,
written in Rust. It reads a `.kicad_pcb`, places and routes it, and writes a
`.kicad_pcb` that KiCad opens unchanged except for what we meant to change. It
runs headless from a CLI; it does not need KiCad installed to place or route
(only to grade, via `kicad-cli`).

Placement comes first. Routing a bad placement well is still a bad board, and
placement is where every free tool stops: Freerouting routes the parts where you
left them. The bar we measure against is a commercial service's returned layout
on the same board (see Corpus).

## Start every session here
1. Read [`ROADMAP.md`](ROADMAP.md) (stage, headline numbers, what's next) and the
   ranked work list in [`docs/gaps.md`](docs/gaps.md). Unless the user gives you a
   task, take the top item.
2. Follow the loop: **orient → plan → implement + test → verify → record → commit.**
   Verify means graded by an external tool on a real board (below), not "it ran".
   Record means the ROADMAP and gaps entries move in the same commit as the code.
3. Keep each step small enough to commit within the hour. A step that only exists
   in your head is lost when the session stops; commit unfinished work on a branch
   rather than holding it.

## Non-negotiables

**1. Never damage a board.**
- Input files are read-only. Output always goes to a new path; refuse to write
  over the input.
- The file layer is lossless: parse → write without edits must reproduce the
  input **byte for byte**. `cargo test` checks this on every corpus board. An
  edit changes only the nodes it means to change; everything else, including
  formatting, keeps its original bytes.
- Never drop a node you do not understand. Unknown tokens pass through untouched.

**2. Ask the part, not the name.** Nothing structural is decided by matching a
net name, a refdes or a pin number. A design does not guarantee names: power
rails are called anything, `U1` means a different part on every board, the USB
pair may be `N$80/N$81`. Decide from what the file actually says: pad electrical
types (`pintype`), net classes, footprint attributes, connectivity, geometry.
A rule keyed on a name does not fail loudly; it matches nothing and the tool
reports a confident answer about the wrong object. If a name is genuinely the
only signal, it is a user-supplied override in a config file, never a literal
in the code.

**3. We do not grade our own work.** "The placer finished" and "the router
finished" are not results. Every claim of improvement comes from tools that did
not produce the board:
- `kicad-cli pcb drc` — clearance violations, unconnected count, schematic parity
  (`"C:/Program Files/KiCad/9.0/bin/kicad-cli.exe"` on this machine);
- the metrics in `autolayout grade`, which re-derive everything from the
  written file read back off disk, not from the placer's in-memory state.
Always report the **unconnected count**. Never let "routed" mean "the router
stopped".

**4. Deterministic.** Same input file + same options + same seed → the same
output bytes. Any randomness takes an explicit seed, printed in the report. A
result nobody can reproduce cannot be compared against.

**5. Never crash.** Board files are untrusted input. Fallible work returns
`Result`; the workspace lints ban `unwrap`, `expect`, `panic!`, `todo!`,
`unimplemented!` and `unreachable!` outside tests, so `./ci.sh` fails on them.
Use `.get(i)` when an index comes from data; cap sizes and depths read from
input; check floats are finite before using them. Tests may unwrap.

**6. Clean-room, licence-clean.** Code is MIT OR Apache-2.0. Never copy code
from GPL/AGPL projects — that includes **KiCad itself and Freerouting** (both
GPL-3). Behaviour comes from public documentation (the KiCad file-format docs at
dev-docs.kicad.org, published papers, textbooks), cited by URL in the module
docstring. Freerouting and KiCad may be *run* as external programs for
comparison; their source is never read into this repo.

**7. Every number comes from somewhere.** A metric in a doc or commit message
names the board, the command, and whether it was measured or estimated. If it
was not measured, say so; do not round a guess up to a fact. If an investigation
only got half an answer, record the half and name the missing half.

## Corpus

Real boards, used read-only and **never committed here** (they carry their own
licences, and they are large):

- `C:/Portcullis/controller/boards/` — 20 boards of an access-control family,
  2- and 4-layer, 50–200 parts. Useful files per board:
  - `quilter/freeplace/<sku>/<sku>.kicad_pcb` — fixed parts (holes, connectors,
    relays) on the outline, everything else on a staging grid: **our input**.
  - `quilter/returned/<sku>/<candidate>/<sku>.kicad_pcb` — the commercial
    service's placed-and-routed result: **the bar**.
  - `quilter/fab/m1/`, `quilter/fab/emu/` — the two boards taken to fab.
  - **Stale boards:** as of 2026-10-12 the r and m2 boards do not match their
    netlists (missing R28/R29 and R94–R97). Don't grade against them until they
    are regenerated.
- Portcullis's own docs record what good looks like on these boards and why:
  its `CLAUDE.md`, `controller/docs/m1-layout-experiment-2026-10-10.md` (the
  measured baseline), and `controller/docs/comprehension-pairing-2026-08-28.md`.
  Read them for the domain; don't copy its code (different licence, AGPL).

`AUTOLAYOUT_CORPUS` points at the corpus root. Tests that need it skip,
printing why, when it is unset, so the repo builds and tests anywhere.

## Layout

```
crates/
  sexpr/   lossless KiCad s-expression parser/writer (byte-exact round-trip)
  board/   typed view over the tree: footprints, pads, nets, outline, layers
  place/   placement: grouping from connectivity, rules, optimisation, legality
  route/   routing (later — see ROADMAP)
  grade/   metrics re-derived from a written file
  cli/     the `autolayout` binary
```

Lower crates never depend on higher ones (`sexpr` ← `board` ← `place`/`route`/
`grade` ← `cli`). Add a crate only when a layer needs one.

## Quality gate

`./ci.sh` before every commit: `cargo fmt --check`, `cargo clippy --all-targets
-- -D warnings` (including the no-panic lints), `cargo test`. Paste its last line
when claiming it passed. A check that exists but is not run is the same as no
check: if you add one, add it to `ci.sh`.

One task per commit, message starting with the ROADMAP id (`M0.2: footprint
iterator`).

## Toolchain on this machine

Rust stable, **GNU toolchain** (`stable-x86_64-pc-windows-gnu`, no Visual Studio
needed); cargo is in `~/.cargo/bin`, which may need adding to `PATH` in a fresh
shell. KiCad 9.0 at `C:/Program Files/KiCad/9.0/`.

## Don't
- Don't claim a check passed without pasting what it printed.
- Don't overwrite an input board, in the corpus or anywhere else.
- Don't commit corpus boards, generated boards, or anything over 1 MB.
- Don't invent a KiCad token, layer name or footprint field. If it is not in the
  file-format docs or a real file, it is not known yet — say so.
