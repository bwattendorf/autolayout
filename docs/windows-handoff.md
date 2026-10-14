# Windows handoff: judge the M7 router with KiCad

Why: the M7 router (branch `m7-router`) has only been checked by its own
model. CLAUDE.md rule 3 says KiCad DRC is the judge, and KiCad is only on the
Windows machine. This file is what to run there, and how to log it so the
cloud session can continue.

## Prompt to paste into a Claude chat/session on the PC

> You are on the Windows machine with KiCad 9.0, Rust (GNU toolchain), Java
> and the Portcullis repo at `C:/Portcullis`. Work in the autolayout repo.
> Follow `docs/windows-handoff.md` exactly. Do not change router code to
> make a number look better. If a script fails, make the smallest fix that
> gets it running, and say what you changed. Report every number with the
> command that produced it. Commit the results file and any script fixes to
> branch `m7-router` and push. Do not open a PR. Never overwrite a board.

## Steps

Set once per shell (adjust paths): 
```
export PATH="$HOME/.cargo/bin:$PATH"
export AUTOLAYOUT_CORPUS=C:/Portcullis/controller/boards
KPY="C:/Program Files/KiCad/9.0/bin/python.exe"
```

1. **Get the code and run the full gate.**
   `git fetch origin && git checkout m7-router && git pull`
   `bash ci.sh > out/ci_last.txt 2>&1; echo "exit=$?"`
   Record the exit code and the last line. With KiCad present the KiCad
   cross-check should run, not print SKIPPED. Run `./ci.sh --kicad-full`
   only if asked (20 min).

2. **Build:** `cargo build --release`.

3. **Prepare the fab m1 board** (rules = its own fab project, the chosen
   primary comparison). Use a fresh scratch dir `W=out/m7`:
   ```
   mkdir -p out/m7
   F=$AUTOLAYOUT_CORPUS/quilter/fab/m1
   ./target/release/autolayout strip $F/m1.kicad_pcb out/m7/m1_fab.kicad_pcb
   cp $F/m1.kicad_pro out/m7/m1_fab.kicad_pro
   ./target/release/autolayout rules out/m7/m1_fab.kicad_pcb
   ```
   `rules` must print track 0.0889 and via 0.2500/0.1500. If it errors, stop
   and report.

4. **Judge our router with KiCad** (the main test):
   ```
   "$KPY" tools/m7_check.py out/m7/m1_fab.kicad_pcb out/m7/ours GND +3V3
   ```
   (`tools/m7_check.py` has never been run; fix only what is broken.) Save
   the JSON it prints. The numbers that matter: `drc.unconnected_after`,
   `drc.violations_by_kind` (especially `clearance`, `hole_clearance`,
   `copper_edge_clearance`, `track_dangling`, `via_dangling`,
   `shorting_items`), segments, vias, track_mm. Open
   `out/m7/ours/*_routed_drc.rpt` and quote the first 5 lines of each
   violation kind with coordinates. Also say whether any violation sits on a
   stub from a plane-tap via to a pad (the router's least-checked copper).

5. **Baseline for the same board and rules: Freerouting.** Confirm the DSN
   carries the fab rules (the earlier runs silently used 0.2 mm defaults):
   ```
   "$KPY" tools/route_check.py out/m7/m1_fab.kicad_pcb out/m7/fr GND +3V3 10
   ```
   Before trusting it, check the DSN in `out/m7/fr/` for the track width and
   clearance (expect about 0.0889 mm, not 0.2) and that the via padstack is
   0.25/0.15. If it shows defaults, the project file was not beside the
   board when pcbnew loaded it: fix that and re-run, and report the problem.
   Record `unconnected_after`, clearance violations, copper, vias, seconds.

6. **Optional second column, the 0.2 mm default rules** (stress case):
   ```
   ./target/release/autolayout route out/m7/m1_fab.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 --track 0.2 --clearance 0.2 --via 0.6/0.3 --iters 60
   ```
   Model-level only; paste the two summary lines.

7. **If a netlist for m1 exists on this machine** (`m1.net`, from
   Portcullis), tell me its path. It is missing from the cloud clone and is
   needed to place our own m1 for the second comparison row. Do not copy
   corpus boards into the repo.

## Log (this is how the cloud session picks it up)

Create `docs/m7-results/YYYY-MM-DD-fab-m1.md` containing:
- git commit hash tested, KiCad version (`kicad-cli --version`), date;
- the commands run and their exit codes;
- a table: ours vs Freerouting: unconnected (KiCad), clearance violations,
  other violation kinds with counts, copper mm, vias, route seconds, rules
  in force;
- the first violation examples (step 4);
- anything that failed, what you changed (diff summary) and why;
- what you did NOT run.
Mark every number "measured" with its command. Do not round. Copy the raw
`*_drc.rpt` files for the ours/Freerouting routed boards (they are small
text; skip any file over 1 MB) into `docs/m7-results/YYYY-MM-DD-fab-m1/`.
Do not commit boards (`.kicad_pcb`), gerbers or anything over 1 MB.

Then: `git add docs/m7-results tools && git commit` (message starts with
`M7.4:`, end with the usual attribution) and `git push origin m7-router`.
Reply with the table and the commit hash.


---

# Round 2 (2026-10-13): re-judge after the fixes, and test the verifier

Why: KiCad's first verdict (docs/m7-results/2026-10-13-fab-m1.md) found 2
unconnected, 2 track_dangling and 2 hole_to_hole on our board. The router now
has fixes for all three and a new `autolayout verify` that is supposed to agree
with KiCad. Round 2 measures both: does KiCad now come back clean, and does the
verifier agree with KiCad on boards it did not help make.

## Prompt to paste

> Same rules as before (docs/windows-handoff.md): follow "Round 2" exactly, do
> not change router or verifier code to make a number match, fix only what stops
> a script running and say what you changed, report every number with its
> command, commit results to branch `m7-router` and push, no PR, never
> overwrite a board. If KiCad and `autolayout verify` disagree, that is the
> finding: report both and the coordinates, do not reconcile them.

## Steps

Same shell setup as round 1 (`PATH`, `AUTOLAYOUT_CORPUS`, `KPY`). `W=out/m7r2`.

1. `git fetch origin && git checkout m7-router && git pull`; record the commit
   hash. `bash ci.sh > out/ci_last.txt 2>&1; echo "exit=$?"` and its last line.
   `cargo build --release`.

2. **Prepare and route (ours), KiCad judges.** Fresh output dir (the router
   refuses to overwrite):
   ```
   mkdir -p out/m7r2
   F=$AUTOLAYOUT_CORPUS/quilter/fab/m1
   ./target/release/autolayout strip $F/m1.kicad_pcb out/m7r2/m1_fab.kicad_pcb
   cp $F/m1.kicad_pro out/m7r2/m1_fab.kicad_pro
   "$KPY" tools/m7_check.py out/m7r2/m1_fab.kicad_pcb out/m7r2/ours GND +3V3
   ```
   Save the JSON. Expected from our side: the router's own `verifier` line says
   0 on every category and the exit code is 0 (see `out/m7r2/ours/*_route.log`).
   What KiCad says is the point: `unconnected_after`, and
   `violations_by_kind` (`track_dangling`, `hole_to_hole`, `clearance`,
   `unconnected_items`, `via_dangling`, `shorting_items`). Quote the first
   five lines of each non-zero kind with coordinates.

3. **Does the verifier agree with KiCad on OUR routed board?**
   ```
   cp out/m7r2/ours/m1_fab_routed.kicad_pcb out/m7r2/ours_check.kicad_pcb
   cp $F/m1.kicad_pro out/m7r2/ours_check.kicad_pro
   ./target/release/autolayout verify out/m7r2/ours_check.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3
   ```
   Put KiCad's count and the verifier's count per category side by side.

4. **Does the verifier agree with KiCad on a board it did not help make?** Use
   the Freerouting result from round 1, or rerun it (10 passes, ~10 min):
   ```
   "$KPY" tools/route_check.py out/m7r2/m1_fab.kicad_pcb out/m7r2/fr GND +3V3 10
   cp $F/m1.kicad_pro out/m7r2/fr/m1_fab_routed.kicad_pro   # verify reads the project beside the board
   ./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb \
       --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3
   ```
   (route_check pours GND on F, In1, B and +3V3 on In2, hence the four
   `--plane` flags.) Round-1 KiCad numbers for Freerouting: 17 unconnected,
   2 clearance, 2 shorting, 45 track_width, 9 via_dangling. The verifier does
   not implement via_dangling or shorting_items (an overlap counts as a
   clearance finding); compare `track_width`, `clearance`, `unconnected`,
   `track_dangling`, `hole_to_hole`. Report where it differs and by how much;
   a verifier that is wrong here has only been fitted to our own board.

5. **Stress case (model only, not KiCad-judged):**
   ```
   ./target/release/autolayout route out/m7r2/m1_fab.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 --track 0.2 --clearance 0.2 --via 0.6/0.3 --iters 60
   ```
   Paste the `negotiated` and `verifier` lines.

6. **Optional, if `m1.net` exists now:** its path. Place our own m1 with
   `autolayout place` and report the command and result; do not commit boards.

## Log

`docs/m7-results/YYYY-MM-DD-fab-m1-round2.md` with: commit hash; KiCad
version; commands and exit codes; table ours-KiCad vs ours-verifier vs
Freerouting-KiCad vs Freerouting-verifier per category; first violation
examples; what you changed (ideally nothing) and what you did not run. Copy the
`*_drc.rpt` and `*_route.log` files (not boards, nothing over 1 MB) into
`docs/m7-results/YYYY-MM-DD-fab-m1-round2/`. Commit, `git push origin
m7-router`, reply with the table and the hash.


---

# Round 3 (2026-10-13): data for the zone-fill verifier

Why: in round 2 KiCad found 17 unconnected items on Freerouting's m1 that
`autolayout verify` missed, because the verifier assumes a plane is solid.
To read zone fills properly I need to see how KiCad 9.0.1 actually writes them.
This round is **read-only data collection: change no code**.

## Prompt to paste

> Follow "Round 3" in docs/windows-handoff.md. Change no code and no boards.
> Run the commands, paste their output into the log file, commit it to
> `m7-router` and push. If a command fails, report the error text; do not fix
> it. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash.
Use the two KiCad-saved boards from round 2 (they are under `out/m7r2/`; if
the directory is gone, rerun round 2 steps 2 and 4 first):

1. `python tools/zone_probe.py out/m7r2/ours/m1_fab_routed.kicad_pcb --show 40`
2. `python tools/zone_probe.py out/m7r2/fr/m1_fab_routed.kicad_pcb --show 40`
   (the Freerouting board has GND zones on F, In1, B and +3V3 on In2.)
3. From Freerouting's DRC report, for each of the 17 `unconnected_items`
   entries, copy the whole entry (they are 3-4 lines each) into the log.
4. For Freerouting's board only: in `kicad-cli`'s output or the pcbnew Python,
   report whether zone fill islands are removed (`zone.GetIslandRemovalMode()`
   for each zone) — one line per zone: layer, net, mode. Under KiCad's python:
   `"$KPY" -c "import pcbnew,sys; b=pcbnew.LoadBoard(sys.argv[1]); [print(z.GetLayerName(), z.GetNetname(), z.GetIslandRemovalMode()) for z in b.Zones()]" out/m7r2/fr/m1_fab_routed.kicad_pcb`
   and the same for the ours board.

## Log

`docs/m7-results/YYYY-MM-DD-zone-data.md`: commit hash, KiCad version, each
command and its exact output (the probe output is small; do not trim it),
the 17 entries, and anything that failed. Commit and `git push origin
m7-router`. Reply with the file name and the hash.


---

# Round 4 (2026-10-13): does the fill-reading verifier now see what KiCad saw?

Why: `autolayout verify` now reads zone `filled_polygon` islands (commit after
813530a). On Freerouting's round-2 board KiCad found 17 unconnected items and
the old verifier 0. This round only runs the verifier on the two existing
boards: no routing, no code change.

## Prompt to paste

> Follow "Round 4" in docs/windows-handoff.md. Change no code and no boards.
> Report KiCad's counts and the verifier's counts side by side, and every
> difference, without reconciling them. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash;
`cargo build --release`. Use the round-2 boards under `out/m7r2/` (each needs
its `.kicad_pro` beside it, as in round 2).

1. Ours:
   `./target/release/autolayout verify out/m7r2/ours_check.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3`
   Expected from KiCad: unconnected 0. Note whether a `LIMIT:` line appears
   (it should not: the fills are in the file) and the "zone fills read" line.
2. Freerouting:
   `./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3 > out/m7r2/verify_fr_round4.txt`
   KiCad's round-2 numbers: unconnected 17 (14 GND: 13 zone-vs-zone + the pad
   pair C2.2/U1.47; 3 +3V3 track pairs), clearance 2, shorting 2, track_width
   45. Paste the verifier's full output.
3. Per net, say whether the verifier's unconnected issues land on the same
   places KiCad's do: GND fragments around (50.425, 50.425), the pad pair at
   (101.7206, 93.8032)/(101.1, 94.85), and the three +3V3 pairs at
   (95.4582, 95.2794), (102.6653, 97.9853), (100.3, 95.1956). The verifier
   prints one location per extra component, which may be a different item of
   the same component; compare counts per net first.

## Log

`docs/m7-results/YYYY-MM-DD-verify-fills.md`: hash, KiCad version, the two
commands and their exit codes, the full verifier output for both boards, a
table of verifier vs KiCad per category and per net (GND, +3V3), and every
difference. Commit and `git push origin m7-router`. Reply with the table and
the hash.


---

# Round 5 (2026-10-13): are the verifier's unconnected parts KiCad's parts?

Why: round 4 matched KiCad's counts (17 = 14 GND + 3 +3V3) but not locations.
The verifier now lists each extra component by its members. This round checks
that the members are the ones KiCad pointed at. No routing, no code change.

## Prompt to paste

> Follow "Round 5" in docs/windows-handoff.md. Change no code and no boards.
> Report which of KiCad's findings fall in which listed part, and every one
> that does not, without reconciling. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash
(it must be later than e85487b); `cargo build --release`.

1. `./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3 > out/m7r2/verify_fr_round5.txt`
   (exit 1 is expected). Paste the whole file into the log. There should now be
   one `unconnected` line per extra part (14 for GND, 3 for +3V3), each with
   `pads [...]`, track count and box, vias, and fill islands.
2. GND pad pair: is `C2.2` in one GND part and `U1.47` in a different one?
   (KiCad: Pad 2 of C2 at (101.7206, 93.8032) and Pad 47 of U1 at (101.1,
   94.85) need a connection.) Say which parts, and whether either part has
   other pads.
3. GND fragments: KiCad lists 13 zone-vs-zone entries. Count the verifier's
   GND parts that contain no pad (fills only, or fills + vias) and say how many
   there are. Expected: 13 fill-only parts if each fragment is its own island.
4. +3V3 pairs: for each KiCad pair, take the two track coordinates —
   (95.4582, 95.2794) and (95.25, 95.5); (102.6653, 97.9853) and (101.8218,
   98.3); (100.3, 95.1956) and (101.3518, 93.357) — and say which verifier
   part's `tracks with ends in (...)` box contains each point, i.e. whether the
   two points of a pair land in two different parts.
5. Anything the verifier lists that KiCad does not, and vice versa.

## Log

`docs/m7-results/YYYY-MM-DD-verify-parts.md` with the hash, the commands and
exit codes, the full verifier output, and the answers to 2-5 as short tables.
Commit and `git push origin m7-router`; reply with the tables and the hash.


---

# Round 6 (2026-10-13): probe KiCad's points against the verifier's parts

Why: round 5 could not tell whether the two points of each KiCad +3V3 pair lie
in two different verifier parts, because the printed boxes overlap. `autolayout
verify --probe X,Y` answers that exactly. No routing, no code change.

## Prompt to paste

> Follow "Round 6" in docs/windows-handoff.md. Change no code and no boards.
> Report what each probe says, and any KiCad item whose probe finds no
> copper or the wrong part, without reconciling. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash
(later than d53fe79); `cargo build --release`.

1. One command (exit 1 is expected), output kept whole:
   ```
   ./target/release/autolayout verify out/m7r2/fr/m1_fab_routed.kicad_pcb \
     --plane In1.Cu=GND --plane F.Cu=GND --plane B.Cu=GND --plane In2.Cu=+3V3 \
     --probe 95.4582,95.2794 --probe 95.25,95.5 \
     --probe 102.6653,97.9853 --probe 101.8218,98.3 \
     --probe 100.3,95.1956 --probe 101.3518,93.357 \
     --probe 101.7206,93.8032 --probe 101.1,94.85 \
     --probe 50.425,50.425 > out/m7r2/verify_fr_round6.txt
   ```
2. A table of KiCad's pairs from the probe lines: pair, the two points, the
   copper found at each (kind, net, layers), and the part each belongs to. Say
   for each +3V3 pair whether its two points are in different parts, and for
   the GND pair (101.7206, 93.8032) / (101.1, 94.85) the same. Say what is at
   (50.425, 50.425).
3. Every probe that found no copper, or copper of a different net than KiCad
   names, listed separately.

## Log

`docs/m7-results/YYYY-MM-DD-verify-probes.md`: hash, the command and exit code,
the whole output, the table, the list from step 3. Commit; `git push origin
m7-router`; reply with the table and the hash.


---

# Round 7 (2026-10-13): place our own m1 and judge it (M7.5)

Why: so far the router has only run on the *service's* placement (fab m1). M7.5
compares placements: ours vs the service's, both routed by our router and by
Freerouting, at the same rules, judged by KiCad. Our placement needs a netlist
(`m1.net`), which is gitignored and was missing on both machines. Nothing in
the router or verifier changes this round.

## Prompt to paste

> Follow "Round 7" in docs/windows-handoff.md. Change no autolayout code. If a
> script fails, report the error text and stop that step; do not fix Portcullis.
> Never overwrite a board; use fresh output directories. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash
(later than 8f2537a); `cargo build --release`.

1. **Make m1.net.** Per Portcullis's CLAUDE.md netlists are generated by SKiDL
   and are gitignored; `controller/boards/_common/run-checks.sh` regenerates
   them (all boards, ~minutes). Find or generate `m1.net` — look for it with
   `find /c/Portcullis -name "m1*.net"` first; if absent, run the regenerate
   step (do not use `--fast`) and say exactly what you ran and its exit code.
   Report the path of `m1.net`. Do not commit it.
2. **Place.** From the freeplace board:
   ```
   F=$AUTOLAYOUT_CORPUS/quilter/freeplace/m1
   mkdir -p out/m7r7
   ./target/release/autolayout place $F/m1.kicad_pcb <path>/m1.net out/m7r7/m1_ours_placed.kicad_pcb
   ```
   Paste its report (legality, overlaps, HPWL). Then run `kicad-cli pcb drc`
   on the placed board and report clearance / courtyard / unconnected counts.
3. **Strip, give it the FAB rules, verify rules load.**
   ```
   ./target/release/autolayout strip out/m7r7/m1_ours_placed.kicad_pcb out/m7r7/m1_ours.kicad_pcb
   cp $AUTOLAYOUT_CORPUS/quilter/fab/m1/m1.kicad_pro out/m7r7/m1_ours.kicad_pro
   ./target/release/autolayout rules out/m7r7/m1_ours.kicad_pcb
   ```
   `rules` must print track 0.0889 and via 0.2500/0.1500 (the same rules as the
   service-placement runs; the freeplace project has coarser ones).
4. **Our router on our placement, judged by KiCad:**
   `"$KPY" tools/m7_check.py out/m7r7/m1_ours.kicad_pcb out/m7r7/router GND +3V3`
   and keep the whole JSON. Then
   `./target/release/autolayout verify out/m7r7/router/m1_ours_routed.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3`
   (copy the `.kicad_pro` beside it first) for the verifier's counts.
5. **Freerouting on our placement:**
   `"$KPY" tools/route_check.py out/m7r7/m1_ours.kicad_pcb out/m7r7/fr GND +3V3 10`
   (about 6-10 minutes; confirm the DSN carries 88.9 as in round 1).
6. **Table** (all KiCad numbers): rows = {service placement + our router,
   service placement + Freerouting, our placement + our router, our placement
   + Freerouting}; columns = unconnected, clearance, shorting, hole_to_hole,
   track_dangling, track_width, copper mm, vias, seconds. The service rows are
   in docs/m7-results/2026-10-13-fab-m1-round2.md; do not re-run them.

## Log

`docs/m7-results/YYYY-MM-DD-m7-5-placements.md`: hash, KiCad version, every
command with exit code, the `place` report, the table, the first violation
examples of any non-zero routing category, what failed, what was not run. Copy
`*_drc.rpt` and `*_route.log` (no boards, nothing over 1 MB). Commit; `git push
origin m7-router`; reply with the table and the hash.


---

# Round 8 (2026-10-13): every service-placed board, and why net N$4 fails

Two jobs. **Part A** runs our router over all 37 service-placed boards in the
corpus (fab, returned, finish, routed; all 4-layer, all with a project file)
and lets KiCad judge each, so we get a comparison across the family, not just
m1. **Part B** finds out why net N$4 stays unconnected on our own m1 placement.
Code since round 7: footprint/pad `(clearance X)` overrides are now honoured by
the router and the verifier; round 8 is the first KiCad judgement of that.

## Prompt to paste

> Follow "Round 8" in docs/windows-handoff.md exactly. Change no autolayout
> code. `tools/m7_batch.py` has never been run: if it breaks, make the smallest
> fix that lets it run, say precisely what you changed, and keep going; do not
> change what it measures. Never overwrite a board; fresh output directories.
> No PR. Do not commit boards, gerbers or anything over 1 MB.

## Setup

`git fetch origin && git checkout m7-router && git pull`; record the hash
(later than 5da1710); `cargo build --release`. Same shell setup as before
(`PATH`, `AUTOLAYOUT_CORPUS`, `KPY`).

## Part A: the whole corpus

1. Smoke test on two boards first (2-3 minutes):
   `"$KPY" tools/m7_batch.py out/m7r8_smoke --limit 2`
   Open `out/m7r8_smoke/summary.md`. If a step failed, fix only what is
   needed, then use a NEW directory name for the next try.
2. Full run, our router only (expect roughly 30-60 minutes; boards fail
   independently):
   `"$KPY" tools/m7_batch.py out/m7r8_all`
   It prints one line per board and rewrites `summary.md`, `results.csv` and
   `results.json` after each. Do not stop it for a failing board.
3. Optional, only after step 2 is done and only if the machine is free: the
   Freerouting baseline on the same boards is 6-12 minutes EACH. Run it for
   the boards where our router left `kicad_unconnected_after` > 0 plus the five
   smallest others, with
   `"$KPY" tools/m7_batch.py out/m7r8_fr --freerouting --boards <globs>` and
   say which boards you chose and why. Do not run it on all 37 unless asked.
4. In the log, a table straight from `summary.md`, and then:
   - boards where `status` is not `ok` and the error text;
   - boards where `kicad_unconnected_after` > 0 or `kicad_clearance` > 0;
   - every board where `verifier_unconnected` or `verifier_clearance` differs
     from KiCad's count (the verifier-vs-KiCad column pair);
   - the totals: boards ok, boards with 0 routing violations in KiCad.

## Part B: net N$4 on our own placement

On `out/m7r7/m1_ours.kicad_pcb` (round 7; if gone, regenerate as in round 7 --
the placement is deterministic) with the fab `.kicad_pro` beside it:

1. `./target/release/autolayout route out/m7r7/m1_ours.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 > out/m7r8_n4_route.txt`
   (use a fresh `--out`-free run; it only prints). Paste the `incomplete:` lines
   and the verifier line.
2. Pad access of the N$4 pads: from the `incomplete:` line take each unreached
   pad `REF.PIN`; for each, give its position (`./target/release/autolayout pads
   out/m7r7/m1_ours.kicad_pcb | grep -P "^REF\tPIN\t"`) and run
   `./target/release/autolayout model out/m7r7/m1_ours.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3`
   once and paste its `pad access` section.
3. For each unreached pad, run `autolayout verify ... --probe X,Y` at the pad
   position on the KiCad-saved routed board from round 7
   (`out/m7r7/router/m1_ours_routed.kicad_pcb`, project beside it) and paste
   which copper is within 0.005 mm. Say which neighbouring parts or tracks
   box the pad in, if you can tell from the probes.
4. No fix this round: only the facts.

## Log

`docs/m7-results/YYYY-MM-DD-round8.md` with: hash; KiCad version; every command
and exit code; Part A tables and lists; Part B outputs; what you changed in
`m7_batch.py` (diff) and why; what you did not run. Copy `summary.md`,
`results.csv`, `results.json` and the `*_drc.rpt` / `*_route.log` / `*_verify.txt`
for any board with non-zero violations (no boards; nothing over 1 MB). Commit;
`git push origin m7-router`; reply with the totals, the worst five boards and
the hash.


---

# Round 9 (2026-10-13): why KiCad counts more clearance violations than the verifier

Round 8 left a question the cloud cannot answer: on 12 boards KiCad reports more
clearance (and some shorting/hole_to_hole) violations than `autolayout verify`.
Either the violations already exist in the service's placement before any copper
is added (pad against pad), which the router did not cause, or they involve
copper the router added and the verifier misses a rule. The files that answer it
are the per-board reports from the round-8 batch, still under `out/m7r8_all/` on
this machine. **No re-run, no code change.**

## Prompt to paste

> Follow "Round 9" in docs/windows-handoff.md. Use the existing round-8 files;
> do not re-run the batch and do not change any code. Report facts and counts;
> do not decide whose fault a violation is. No PR.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash (later
than abe3432).

For each of these boards (their folders are under `out/m7r8_all/`, named from the
path with `__` for `/`): `returned/c2/3p5mil-candidate2`,
`returned/c4/3p5mil-candidate3`, `returned/c4/candidate2`,
`returned/emu/candidate2`, `returned/m1/candidate4`, `returned/m1/candidate6`,
`returned/m1/locked-candidate5`, `returned/m1/tp18-candidate2`,
`returned/r/3p5mil-candidate2`, `finish/g2i`:

1. `python tools/drc_compare.py <dir>/b_prepared_drc.rpt <dir>/b_routed_drc.rpt --kind clearance --show 10`
   Paste the per-kind table and the listed new clearance entries. Then the same
   with `--kind shorting_items` and `--kind hole_to_hole` where either is non-zero.
   (`new` counts for silk/unconnected kinds can be inflated by shifted
   coordinates; ignore those and say so.)
2. For the new clearance entries, classify each as: **track/via vs track/via**,
   **track/via vs pad**, **pad vs pad**, **zone involved**, or **other**, and give
   the counts per board. Say which pads' footprints appear most (for example J10).
3. For `finish/g2i` only: paste `<dir>/b_verify.txt` and the DRC
   `unconnected_items` entries from `b_routed_drc.rpt`, since the verifier says 10
   and KiCad 1.

Also, before anything else, copy these two small things into the repo log: the
`results.csv` from the full round-8 run is already committed; add the per-board
`*_prepared_drc.rpt` and `*_routed_drc.rpt` ONLY for the ten boards above if each
is under 1 MB (otherwise paste the entries of the kinds listed and name the file
sizes).

## Log

`docs/m7-results/YYYY-MM-DD-round9.md`: hash; per-board tables from step 1; the
classification table from step 2; the g2i material; what you did not do. Commit;
`git push origin m7-router`; reply with the classification totals and the hash.


---

# Round 10 (2026-10-13): KiCad judges the round-9 fixes

Since round 8 the router and verifier changed in five ways, each tied to a
KiCad finding: dropped nets are re-routed in hard mode; custom pads (board r's
U6) are modelled from their polygon primitives; a `max_error` (0.005 mm)
clearance margin; off-grid stubs claim a corridor so other nets stay clear of
them; and a pad joins a same-net fill that stops at its edge (thermal relief,
the g2i hypothesis). None of it has been judged by KiCad. `tools/m7_batch.py`
now also records clearance/shorting/hole_to_hole BEFORE routing, so new
violations are `after - before`.

## Prompt to paste

> Follow "Round 10" in docs/windows-handoff.md exactly. Change no autolayout
> code. If `tools/m7_batch.py` breaks, make the smallest fix, say what you
> changed, keep going. Fresh output directories; never overwrite a board; no
> PR; no boards or files over 1 MB committed.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash (later
than 98cdb93); `cargo build --release`. Same shell setup as before.

1. Whole corpus again, our router, new directory:
   `"$KPY" tools/m7_batch.py out/m7r10_all`
2. A table straight from `summary.md`, then these comparisons against round 8
   (`docs/m7-results/2026-10-13-round8/results.csv`), per board:
   unconnected, and **new** clearance/shorting/hole_to_hole =
   `kicad_<kind>` minus `kicad_<kind>_before`. Totals of each, round 8 vs now.
   Boards that got worse on any of them, listed with the numbers.
3. The two checks the fixes were made for:
   - `returned/r/3p5mil-candidate2`: new clearance and new shorting (round 8:
     2 and 2 new). Expect 0 and 0; say what KiCad reports.
   - `finish/g2i`: KiCad unconnected vs `verifier_unconnected` (round 8: 1 vs
     10). Expect the verifier to say 1 too; if it still lists the nine GND
     connector pads, run
     `./target/release/autolayout verify <dir>/b_routed.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 --probe 58.82,84.1`
     and, with pcbnew, print for pad J2.2 `GetZoneConnection()` and whether the
     routed board's GND fills reach it; report both.
4. Verifier vs KiCad: boards where `verifier_unconnected` or the verifier's
   new-clearance count differs from KiCad's, with the numbers.
5. Optional, only after 1-4 are logged: Freerouting on six representative
   boards (the service's own final candidates), each 6-12 minutes:
   `returned/m1/3p5mil-candidate1`, `returned/c2/3p5mil-candidate2`,
   `returned/emu/3p5mil-candidate3`, `returned/g2/3p5mil-candidate6`,
   `returned/io/3p5mil-candidate4`, `returned/t/3p5mil-candidate6`:
   `"$KPY" tools/m7_batch.py out/m7r10_fr --freerouting --boards quilter/returned/m1/3p5mil-candidate1/m1.kicad_pcb ...`
   and the same columns for Freerouting from the `route_check.py` JSON
   (`unconnected_after_routing`), against our router's numbers on those boards.

## Log

`docs/m7-results/YYYY-MM-DD-round10.md`: hash, commands and exit codes, the
tables from 2-4, the answers to 3 and 4, step 5 if run, what changed in the
script, what was not run. Copy `summary.md`, `results.csv`, `results.json`. Commit;
`git push origin m7-router`; reply with the new totals, any board that got worse,
and the hash.


---

# Round 11 (2026-10-13): judge the current branch (stub-length change) and one plane check

Round 10 ran on `d0a443e`. Commit `8d610bc` (stubs end just inside the pad edge)
and later improve m1-2026-08-06-candidate5 (local: 77 -> 82 of 82 nets), g2i
(25 -> 26 of 32, unconnected 8 -> 6) and m1 candidate4 (unconnected 25 -> 12),
by the verifier. This round has KiCad judge them. No code change.

## Prompt to paste

> Follow "Round 11" in docs/windows-handoff.md exactly. Change no autolayout
> code. If `tools/m7_batch.py` breaks, make the smallest fix and say what you
> changed. Fresh output directories; never overwrite a board; no PR; no boards
> or files over 1 MB committed.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash
(must be later than c2c4c73 and include `8d610bc`); `cargo build --release`.

1. `"$KPY" tools/m7_batch.py out/m7r11_all`
2. Totals vs round 10 (`docs/m7-results/2026-10-13-round10/results.csv`):
   boards with 0 routing violations; boards with 0 unconnected and 0 new
   violations; KiCad unconnected summed; new clearance / shorting / hole_to_hole
   summed; nets complete / total. Then per board, only those where KiCad
   unconnected or any new violation changed, with both numbers.
3. The four boards the change was aimed at, KiCad numbers: `returned/m1/candidate4`,
   `finish/g2i`, `returned/m1-2026-08-06-candidate5`, `returned/c4/3p5mil-candidate3`.
4. Plane islands (the r and c4/3p5 findings): for `returned/r/3p5mil-candidate2`
   and `returned/c4/3p5mil-candidate3`, list KiCad's `unconnected_items` and
   `via_dangling` entries (whole), and run
   `./target/release/autolayout verify <dir>/b_routed.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 --probe X,Y`
   at each via/pad coordinate KiCad lists, pasting what the probe finds and
   which fill (layer, part) covers the point. Say whether the via sits in a
   fill polygon that holds no other pad.

## Log

`docs/m7-results/YYYY-MM-DD-round11.md`: hash, commands and exit codes, the
totals comparison, the per-board changes, the four-board table, the step 4
entries and probes, what was not run. Copy `summary.md`, `results.csv`,
`results.json`. Commit; `git push origin m7-router`; reply with the new totals,
any board that got worse, and the hash.

---

# Round 12: plane pockets against KiCad, placement by routability, Freerouting on c2 and io

Commits since round 11 (`ea961e6`): `9a2b5b6` (stub landing reach 1.5 -> 4.5
pitches; local verifier unconnected 209 -> 194), `5695b5d` (plane-pocket
emulation in `verify`), `6d0767e` (Freerouting tooling), and the
placement-comparison script. Every number below the branch produces is
UNVERIFIED until this round.

## Prompt to paste

> Follow "Round 12" in docs/windows-handoff.md exactly. Change no autolayout
> Rust code. If a script in `tools/` breaks, make the smallest fix and say what
> you changed (m7_place_compare.py and the close_outline function in
> route_check.py have never run). Fresh output directories; never overwrite a
> board; no PR; no boards or files over 1 MB committed. If something is
> missing (a netlist, Java), say so and do the other steps.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash (must
include `6d0767e`); `cargo build --release`.

1. `"$KPY" tools/m7_batch.py out/m7r12_all`. Totals vs round 11
   (`docs/m7-results/2026-10-13-round11/results.csv`): boards with 0 routing
   violations; boards with 0 unconnected and 0 new violations; KiCad
   unconnected summed; new clearance / shorting / hole_to_hole summed; nets
   complete / total; then per board where KiCad unconnected or any new
   violation changed, both numbers.
2. Plane pockets, the calibration. Each board dir holds `*_verify.txt` (the new
   verifier) and the KiCad DRC report. For EVERY board, one row:
   `emulated pockets (count and the plane_island coordinates)` against
   `KiCad unconnected_items / via_dangling entries that name a plane net (+3V3
   or the ground net), with coordinates`. Say per board: both agree, only the
   emulation, only KiCad. Local expectation (our verifier, NOT KiCad): 1 on
   c4/3p5mil-candidate3 at (132.0772, 73.0328), 2 on r/3p5mil-candidate2,
   1 each on c2, el, emu and g2 3p5mil, 0 elsewhere. For each emulated pocket
   KiCad does not report, run
   `./target/release/autolayout verify <dir>/b_routed.kicad_pcb --plane In1.Cu=GND --plane In2.Cu=+3V3 --probe X,Y`
   and paste the probe, and say whether KiCad's fill covers the point
   (`tools/zone_probe.py`).
3. Placement by routability (needs the Portcullis netlist; m1 is
   `controller.net`, find the others). For m1: freeplace board
   `quilter/freeplace/m1/m1.kicad_pcb`, returned board = the best service
   candidate of round 11 (name it), then
   `python tools/m7_place_compare.py FREEPLACE NETLIST RETURNED out/m7r12_place_m1 --pro <the m1 .kicad_pro> --plane In1.Cu=GND --plane In2.Cu=+3V3 --variant nohalo=--no-halo --judge GND +3V3`
   (judge needs KiCad's python as `sys.executable`; if it does not, run
   `tools/m7_check.py` by hand on each `*_stripped.kicad_pcb`). Paste the
   table and the KiCad numbers. Repeat on one more board with a netlist if one
   is cheap (c4 or emu).
4. Freerouting on the two boards that gave no number. c2:
   `"$KPY" tools/m7_batch.py out/m7r12_c2 --boards "quilter/returned/c2/**/*.kicad_pcb" --freerouting`
   (the DSN export now closes a broken outline in memory; paste the `outline:`
   line from stderr/log and whether a DSN was written and what Freerouting
   left unconnected). io:
   `"$KPY" tools/m7_batch.py out/m7r12_io --boards "quilter/returned/io/**/*.kicad_pcb" --freerouting --fr-timeout 14400 --fr-passes 5`
   Report seconds, whether `timed_out` is true, KiCad unconnected after.

## Log

`docs/m7-results/YYYY-MM-DD-round12.md`: hash, commands and exit codes, step 1
totals, the step 2 table (every board), step 3 tables and judge output, step 4
results, what was not run. Copy `summary.md`, `results.csv`. Commit; `git push
origin m7-router`; reply with the new totals, the agree / emulation-only /
KiCad-only counts for step 2, and the hash.

---

# Round 13: KiCad judges the stub-reach portfolio and the calibrated pockets

Run after Round 12 step 4 (Freerouting c2 / io) has finished, so the two do
not fight over the machine. Commits since Round 12's tested `a19543f`:
`f93c772` (route tries stub reach 1.5, then 4.5 only if incomplete, keeps the
one with fewer verifier-unconnected) and `8b4cf00` (pocket emulation with a
circular erosion element; cut-off taps leave connectivity). Local verifier
expectation, NOT KiCad: unconnected 163 over the 37 boards (Round 12 KiCad:
196), pockets exactly c4/3p5mil-candidate3 and r/3p5mil-candidate2.

## Prompt to paste

> Follow "Round 13" in docs/windows-handoff.md exactly. Change no autolayout
> code. Fresh output directories; never overwrite a board; no PR; no boards or
> files over 1 MB committed.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash
(must include `8b4cf00`); `cargo build --release`.

1. `"$KPY" tools/m7_batch.py out/m7r13_all`. Totals vs Round 12 and vs Round 11
   (boards with 0 routing violations; 0 unconnected and 0 new violations; KiCad
   unconnected summed; new clearance / shorting / hole_to_hole; nets complete),
   then every board where KiCad unconnected changed vs Round 11, both numbers.
   Specifically the nine boards that moved in Round 12 (the five m1 returns and
   g2i, c4/candidate2, emu/candidate2, tp18-candidate2/3).
2. Does the verifier still equal KiCad? Per board, verifier `unconnected` vs
   KiCad `unconnected`; list every board where they differ. The batch verifies
   the KiCad-saved file, which carries fills, so the pocket emulation does not
   run there; for the pocket check use the fill-stripped copies as in Round 12
   (`out/m7r12_nofill_b` method) on c4/3p5mil-candidate3 and r/3p5mil-candidate2
   and say whether the verifier's `unconnected` equals KiCad's (expected 2 and 1).
3. Timing: seconds per board for the five slowest boards vs Round 12 (the
   portfolio may run each twice).

## Log

`docs/m7-results/YYYY-MM-DD-round13.md`: hash, commands and exit codes, the
totals, the changed-board table, the step 2 and 3 tables, what was not run.
Copy `summary.md`, `results.csv`. Commit; `git push origin m7-router`; reply
with the new totals and the hash.

---

# Round 14: find the c2 DSN failure, placement on more boards, one open island

Cloud-side findings that shape this round (read from the c2 board file, not
KiCad): c2's Edge.Cuts is exactly four `gr_line`s forming a closed rectangle,
all ends identical, so the Round 12 "non-closed outline" assertion
(`SHAPE_POLY_SET::AddOutline`) is NOT the board outline. Something else in the
DSN export is an open polygon: a zone or rule area, a custom pad primitive, a
footprint graphic, or a courtyard. Round 12's batch also hung 3600 s while the
direct run aborted in 3.8 s; a modal assertion dialog waiting for a click is the
likely cause. No new autolayout code is needed for any step.

## Prompt to paste

> Follow "Round 14" in docs/windows-handoff.md exactly, then the Backlog
> section below it if you have time left. Change no autolayout Rust code. Fresh
> output dirs; never overwrite a board; no PR; nothing over 1 MB committed.

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash;
`cargo build --release`.

1. c2 DSN bisect. Take the stripped c2 board from Round 12
   (`out/m7r12_c2_direct`, or `autolayout strip` the c2 return again). Write a
   small script (keep it under `out/`, do not commit it unless it is generally
   useful; if so put it in `tools/dsn_bisect.py`) that, for each variant, loads
   the board in KiCad's Python, applies the variant, calls
   `pcbnew.ExportSpecctraDSN` in a SUBPROCESS with a 60 s timeout (an assertion
   aborts the process; a dialog hangs it), and records exit code and time:
   A as is; B with all zones removed; C with every footprint's non-pad
   graphics removed; D with every footprint's custom pads replaced by their
   anchor shapes; E with every rule area/keepout removed. Then bisect the
   footprints (halves) on whichever variant first exports cleanly, down to the
   one footprint or zone that triggers it. Report: the variant table, the
   offending item (reference, pad number, zone name or layer, or "not found"),
   and what about it is open (for a pad: the `gr_poly` points).
2. If step 1 found the cause and a one-line in-memory workaround is obvious
   (e.g. drop that footprint's courtyard for the export only), apply it to
   `tools/route_check.py` (smallest change), run
   `"$KPY" tools/m7_batch.py out/m7r14_c2 --boards "quilter/returned/c2/**/*.kicad_pcb" --freerouting --fr-timeout 14400 --fr-passes 5`
   and report Freerouting's KiCad unconnected on c2 beside ours (0).
3. Placement on more boards. For each of c4, emu and one more board that has a
   netlist (find them under the Portcullis tree), run
   `python tools/m7_place_compare.py FREEPLACE NETLIST RETURNED out/m7r14_place_<sku> --pro <.kicad_pro> --plane In1.Cu=GND --plane In2.Cu=+3V3 --variant nohalo=--no-halo --judge GND +3V3`
   and give the table per board. For c4 explain the footprint counts (ours 307,
   service 299): list the references present in one board and not the other.
4. m1/locked-candidate5: KiCad reports `unconnected_items` Pad 1 [+3V3] of C6
   (103.83, 98.3) to Pad 38 [+3V3] of U1 (102.15, 98.3), and our emulation
   found no pocket there. With `autolayout verify --probe` on that board's
   routed file at both points, and `tools/zone_probe.py`, say what separates
   the two pads in KiCad: a fill gap (and between which copper), a missing
   track, or something else.

## Log

`docs/m7-results/YYYY-MM-DD-round14.md`: hash, commands and exit codes, the
step 1 variant table and offender, step 2 result or why skipped, step 3 tables,
step 4 answer, what was not run. Commit; `git push origin m7-router`.

---

# Backlog (do these when no round is pending)

Do the first item whose condition holds; log it under
`docs/m7-results/YYYY-MM-DD-backlog-<item>.md`; commit and push; stop after one.

B1. Regression sweep. If `origin/m7-router` has commits touching `crates/` newer
    than the commit of the latest `roundN` log or backlog sweep, run
    `"$KPY" tools/m7_batch.py out/m7b1_<hash>` and compare to the newest
    `results.csv` in `docs/m7-results/`: totals, and every board whose KiCad
    unconnected or any new violation changed. Say "worse" or "not worse".
B2. Variance. Run the same batch a second time on the same commit; report any
    board whose KiCad unconnected differs between the two runs (the router is
    deterministic, so any difference is a tooling or KiCad problem) and the
    summed `route_s` of both.
B3. Timing. For the five slowest boards, report `route_s` three times each.

---

# Round 15: reconcile the placement table, run c2 Freerouting once, close Round 14

Decisions made for you (do not reopen them): (1) J90 narrowing is dropped. The
assertion is a warning, the DSN is written, and Round 14 spent ~60 dialogs on
it. Do not run any more bisect exports. (2) The c2 Freerouting run is on: start
it FIRST, in the background, and accept the assertion dialogs of that one
export (4 of them). (3) Round 14's step 3 table is NOT comparable with
Rounds 12-13 and has to be redone: for c4 the service row says 119/190 nets and
85 KiCad unconnected, while Round 13 has the same board at 189/190 nets and 2
unconnected, and Round 12's m7_place_compare gave 2. A rules mismatch is the
likely cause (a wrong or default `.kicad_project`/`--pro` for the stripped
returned board).

## Prompt to paste

> Follow "Round 15" in docs/windows-handoff.md exactly. You are in charge of
> this round: docs/windows-agent.md, section "You are in charge of this round".

## Steps

`git fetch origin && git checkout m7-router && git pull`; record the hash;
`cargo build --release`.

1. Start c2 Freerouting in the background now:
   `"$KPY" tools/m7_batch.py out/m7r15_c2 --boards "quilter/returned/c2/**/*.kicad_pcb" --freerouting --fr-timeout 14400 --fr-passes 5`
   Redirect output to a file; check on it at the end, not repeatedly.
2. Redo the placement table for c4, emu and g2 with the RIGHT rules. For each
   board find the `.kicad_pro` that sits beside the returned board you compare
   against (not the one beside the freeplace board, not a copy from another
   SKU) and pass it as `--pro`. First check the service row: it must match
   Round 13's numbers for that board (nets complete and KiCad unconnected from
   `docs/m7-results/2026-10-14-round13/results.csv`). If it does not, say what
   differs (rules file, plane nets, layer count) and fix the command until it
   does, then run ours default and ours nohalo with the same `--pro`. Give the
   table with footprints, off-board, nets complete, KiCad unconnected, KiCad
   clearance, track mm, vias.
3. c4 footprints: ours has D29, D34, D35, D36, D38, D39, D40, D41 and the
   service board does not. Look each up in c4's netlist and BOM/schematic
   (populate flags, DNP, `exclude_from_board`) and say whether they are real
   parts the service dropped or parts that should not be on the board.
4. When the c2 job finishes, report Freerouting's KiCad unconnected on c2
   beside ours (0), seconds, and whether it timed out.

## Log

`docs/m7-results/YYYY-MM-DD-round15.md`: hash, commands and exit codes, the
step 2 service-row reconciliation, the table, step 3 answer, step 4 result
(or "still running at <time>, output in <path>" if you must write before it
ends, then append when it does). Commit; `git push origin m7-router`.

---

# Round 16: is the m1 MCU-pin ceiling the placement or our router?

Why: the weak m1 returns (candidate6 20, doublesided-candidate4 15,
locked-candidate5 29 KiCad unconnected) are mostly unrouted MCU (U1) pins. If
Freerouting also leaves U1 pins unrouted on the same boards, the ceiling is the
placement, and no router change will move it. If Freerouting escapes them, our
router is leaving routes on the table. This decides what the cloud session
works on next.

## Prompt to paste

> Follow "Round 16" in docs/windows-handoff.md exactly. You are in charge of
> this round (docs/windows-agent.md). Start the Freerouting jobs in the
> background first.

## Steps

`git pull`; record the hash; `export AUTOLAYOUT_CORPUS=C:/Portcullis/controller/boards`.

1. Background jobs, one batch each, 10 passes, 14400 s limit, output to files:
   `"$KPY" tools/m7_batch.py out/m7r16_<name> --boards "quilter/returned/m1/<dir>/*.kicad_pcb" --freerouting --fr-timeout 14400 --fr-passes 10`
   for `<dir>` = `candidate6`, `locked-candidate5`, `doublesided-candidate4`.
2. For each board, when it finishes: Freerouting's KiCad unconnected, seconds,
   timed out or not, beside our router's KiCad unconnected from Round 13
   (20, 29, 15). Then LIST the pads still unconnected in KiCad's report for
   both routers (reference.pin and net) and say how many are U1 pins in each.
3. Say in one line per board which it is: both leave U1 pins unrouted (placement
   ceiling), only ours does (router gap), or neither.

## Log

`docs/m7-results/YYYY-MM-DD-round16.md`; append each board as it ends. Commit;
`git push origin m7-router`.

---

# Round 17: placement by routability on every board that has a netlist

Why: m1, c4, emu and g2 are at parity (Rounds 12 and 15). Thirteen more boards
may have netlists; one result per board is not a placer yet.

## Prompt to paste

> Follow "Round 17" in docs/windows-handoff.md exactly. You are in charge of
> this round (docs/windows-agent.md).

## Steps

1. List every SKU with all three of: a freeplace board, a netlist, and a
   returned board (use the newest `3p5mil-candidate*` return when there are
   several). Skip c4, emu, g2 and m1 (done). Write the list to the log.
2. For each, `python tools/m7_place_compare.py FREEPLACE NETLIST RETURNED out/m7r17_<sku> --pro <.kicad_pro beside the RETURNED board> --plane In1.Cu=GND --plane In2.Cu=+3V3 --judge GND +3V3`
   (default halo only; `--variant nohalo=--no-halo` is not needed). Before
   trusting a board, check its service row against Round 13's `results.csv`
   (nets complete and KiCad unconnected); if it differs, fix the command (rules
   file, plane nets) until it matches, or log the board as blocked with the
   exact difference.
3. One table: sku, footprints ours/service, off-board ours, nets complete
   ours/service, KiCad unconnected ours/service, KiCad clearance ours/service,
   track mm ours/service. Then one line: how many boards are within 2
   unconnected of the service, and the worst three.
4. For every board where ours leaves parts off the board, list the references.

## Log

`docs/m7-results/YYYY-MM-DD-round17.md`; commit; `git push origin m7-router`.
