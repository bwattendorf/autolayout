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
