# Using autolayout on a board

Everything writes a NEW file; an input board is never touched and an existing output is never overwritten.
Rules (track, clearance, vias) come from the `.kicad_pro` beside the board: the router refuses to guess,
so put the project file you want next to the board (the manufacturing rules, not generic defaults: see
"Rules" below). Numbers quoted here are measured, with the board and command named in
`docs/debug-logs/2026-10-17-night.md` and ROADMAP.

## Route a board that already has its parts placed

```
autolayout route BOARD.kicad_pcb --auto-planes --out ROUTED.kicad_pcb
```

- `--auto-planes` picks the plane nets for the inner layers from pad counts (never from names); give
  `--plane LAYER=NET` yourself to override (`autolayout planes BOARD` prints what it would choose).
- The router tries several stub/corridor settings in parallel and keeps the best by its own verifier;
  `--quick` runs one attempt (minutes become seconds, a little weaker), `--iters N` bounds the negotiation,
  `--seed N` varies the net order (0 = default), `--explain` says why a pad stayed unreached.
- Judge the result with KiCad, not with the router: `kicad-cli pcb drc --refill-zones --severity-error
  ROUTED.kicad_pcb` (zones must be refilled, or plane-connected pads count as open). The verifier line
  in the router output is its own estimate.
- Same input, options and seed give the same bytes (checked: `tools/determinism.py`).

## Place and route a board from a staging grid

```
autolayout place FREEPLACE.kicad_pcb NETLIST.net PLACED.kicad_pcb [--grade] [--spread] [--halo-scale X]
autolayout route PLACED.kicad_pcb --auto-planes --out ROUTED.kicad_pcb
```

- The placer reads the netlist (pad nets and pin types), leaves locked parts where they are, and places
  the rest. A locked part with a pad outside the outline cannot stay, so it is placed and named in the
  output.
- `--grade` places by connection grade (hubs first); the default is the one measured below.

## Rules

The freeplace project files in the Portcullis corpus carry generic defaults (track 0.25, clearance 0.2,
via 0.6/0.3); the boards are made at the service's 3.5 mil rules (0.0889, via 0.25/0.15). Routing a
fine-pitch part at the coarse rules is nearly impossible (m1 placed by us: 65 unconnected at the
coarse rules, 0 at the fab rules). Route and judge under the rules the board will be made with.

## Measured state (2026-10-18, KiCad 10 with zones refilled, one run per board)

- Our placer + our router, fab rules: 19 of 19 freeplace boards with a netlist reach 0 unconnected with
  no new clearance/shorting/hole-to-hole violations (`tools/place_sweep.py`).
- Our router on the service's placements, fab rules: 0 unconnected on every 3.5 mil board; the 6 mil
  candidates c4/candidate2, emu/candidate2 and m1/candidate4 are not closed (12, 19 and 10 open; the
  service leaves pads open on them too).

## Sweeps and gates (all KiCad-judged, resumable, deterministic)

- `tools/overnight.py OUT --hours H`: ladder of router settings over every corpus board.
- `tools/place_sweep.py OUT --hours H [--boards ID ...] [--check]`: our placer + router on the freeplace
  boards; `--check` sets the exit status (the regression gate).
- `tools/determinism.py BOARD_ID OUT`: place and route twice, compare the bytes.
- Run these under KiCad's Python (`"C:/Program Files/KiCad/10.0/bin/python.exe" tools/...`).
