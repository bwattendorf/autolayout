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
