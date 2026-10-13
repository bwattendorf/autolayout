"""Run the M7 router over many service-placed boards and let KiCad judge each.

Run under KiCad's bundled Python (needs pcbnew), after `cargo build --release`:

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/m7_batch.py OUT_DIR \
        [--boards GLOB ...] [--skip SUBSTR[,SUBSTR...]] [--limit N] [--freerouting] [--fr-timeout SECONDS] [--fr-passes N]

OUT_DIR must not exist yet (nothing is ever overwritten). Boards default to
every `.kicad_pcb` under $AUTOLAYOUT_CORPUS/quilter/{fab,returned,finish,routed}
(37 files on 2026-10-13, all 4-layer, all with a project file beside them).
`--boards` takes globs relative to the corpus root instead. `--skip` drops any
board whose relative path contains one of the substrings (a user override;
CLAUDE.md says the r and m2 boards are stale against their netlists, which does
not matter for routing but is worth knowing when reading the table).

Per board, in its own folder under OUT_DIR:
  1. `autolayout strip` the copper, keep the project file beside it;
  2. `autolayout planes` picks the plane nets from pad counts (never from names);
  3. pcbnew pours those planes on the inner layers and fills them -> *_prepared;
  4. `kicad-cli pcb drc` on it: the unconnected count BEFORE routing;
  5. `autolayout route ... --out` (our router; it also runs its own verifier);
  6. pcbnew refills the zones around the new copper -> *_routed;
  7. `kicad-cli pcb drc` again: THE VERDICT, with every violation kind counted;
  8. `autolayout verify` on the routed file (verifier vs KiCad, same board);
  9. with --freerouting, tools/route_check.py on the same stripped board
     (10 passes, slow: minutes per board).
A failing step is recorded in the row and the batch moves on. Written after
each board: results.csv, results.json, summary.md.

STATUS: written 2026-10-13 in a cloud session with no KiCad; the pcbnew steps
have never been run. The first run on a board or two may need small fixes.
Judge by KiCad's numbers, not by anything this script or autolayout prints.
"""

import csv
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import time

import pcbnew

KICAD_CLI = os.environ.get("KICAD_CLI", r"C:/Program Files/KiCad/9.0/bin/kicad-cli.exe")
AUTOLAYOUT = os.environ.get("AUTOLAYOUT", os.path.join("target", "release", "autolayout.exe"))
HERE = os.path.dirname(os.path.abspath(__file__))

COLUMNS = [
    "board", "status", "planes", "nets_total", "nets_complete", "pads_connected", "pads_total",
    "route_s", "segments", "vias", "track_mm",
    "kicad_unconnected_before", "kicad_unconnected_after",
    "kicad_clearance_before", "kicad_shorting_items_before", "kicad_hole_to_hole_before",
    "kicad_clearance", "kicad_shorting_items", "kicad_hole_to_hole", "kicad_track_dangling",
    "kicad_via_dangling", "kicad_track_width", "kicad_copper_edge_clearance", "kicad_hole_clearance",
    "kicad_other_kinds",
    "verifier_unconnected", "verifier_clearance", "verifier_shorting", "verifier_hole_to_hole",
    "verifier_track_dangling", "verifier_track_width",
    "freerouting_unconnected_after", "freerouting_s", "error",
]

# DRC kinds that are not about routing: identical before and after, reported apart.
NON_ROUTING = {"courtyards_overlap", "silk_edge_clearance", "lib_footprint_issues", "silk_over_copper",
               "silk_overlap", "text_height", "text_thickness", "footprint_symbol_mismatch",
               "missing_footprint", "extra_footprint", "duplicate_footprints", "padstack"}


# Freerouting bounds: --fr-timeout SECONDS (default 3600) and --fr-passes N
# (default 10). io timed out at the old fixed 3600 s in round 8; a longer
# bound or fewer passes is now a flag, and a timeout is reported as one.
FR_TIMEOUT = 3600
FR_PASSES = 10


def run(cmd, timeout=None, cwd=None, env=None):
    t = time.time()
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd=cwd, env=env,
                       encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr, time.time() - t


def drc(pcb, report):
    run([KICAD_CLI, "pcb", "drc", "-o", report, pcb], timeout=900)
    text = open(report, encoding="utf-8", errors="replace").read()
    m = re.search(r"Found (\d+) unconnected", text)
    kinds = {}
    for k in re.finditer(r"^\[([a-z_]+)\]", text, re.M):
        kinds[k.group(1)] = kinds.get(k.group(1), 0) + 1
    return (int(m.group(1)) if m else None), kinds


def pour(board, assignments, inset_mm=0.5):
    bb = board.GetBoardEdgesBoundingBox()
    x0, y0 = pcbnew.ToMM(bb.GetLeft()) + inset_mm, pcbnew.ToMM(bb.GetTop()) + inset_mm
    x1, y1 = pcbnew.ToMM(bb.GetRight()) - inset_mm, pcbnew.ToMM(bb.GetBottom()) - inset_mm
    for layer_name, net_name in assignments:
        net = board.FindNet(net_name)
        layer = board.GetLayerID(layer_name)
        if net is None or layer < 0:
            raise RuntimeError(f"cannot pour {net_name!r} on {layer_name!r}")
        z = pcbnew.ZONE(board)
        z.SetLayer(layer)
        z.SetNet(net)
        z.SetLocalClearance(pcbnew.FromMM(0.25))
        z.SetMinThickness(pcbnew.FromMM(0.2))
        z.SetPadConnection(pcbnew.ZONE_CONNECTION_FULL)
        o = z.Outline()
        o.NewOutline()
        for x, y in ((x0, y0), (x1, y0), (x1, y1), (x0, y1)):
            o.Append(pcbnew.FromMM(x), pcbnew.FromMM(y))
        board.Add(z)
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())


def with_project(src_pro, dst_pcb):
    shutil.copyfile(src_pro, os.path.splitext(dst_pcb)[0] + ".kicad_pro")


def parse_summary(text, key):
    """Counts from `autolayout` summary lines like 'unconnected 2, track_dangling 0, ...'."""
    m = re.search(key + r" (\d+)", text)
    return int(m.group(1)) if m else None


def do_board(src, work, freerouting):
    row = {c: "" for c in COLUMNS}
    row["board"] = os.path.relpath(src, CORPUS).replace("\\", "/")
    os.makedirs(work)
    stem = os.path.join(work, "b")
    src_pro = os.path.splitext(src)[0] + ".kicad_pro"
    if not os.path.exists(src_pro):
        row["status"], row["error"] = "SKIP", "no .kicad_pro beside the board; the router refuses to guess rules"
        return row
    steps = []

    def step(name, fn):
        try:
            return fn()
        except Exception as e:  # keep going to the next board
            row["status"], row["error"] = "FAIL:" + name, str(e)[:300]
            return None

    stripped = stem + "_stripped.kicad_pcb"
    code, out, _ = run([AUTOLAYOUT, "strip", src, stripped])
    if code != 0:
        row["status"], row["error"] = "FAIL:strip", out.strip()[:300]
        return row
    with_project(src_pro, stripped)

    code, out, _ = run([AUTOLAYOUT, "planes", stripped])
    if code != 0:
        row["status"], row["error"] = "FAIL:planes", out.strip()[:300]
        return row
    flags, pairs = [], []
    for line in out.splitlines():
        if line.startswith("FLAGS\t"):
            flags = [f for f in line.split("\t")[1:] if f]
        m = re.match(r"\s+plane (\S+) = (.*) \(\d+ pads\)$", line)
        if m:
            pairs.append((m.group(1), m.group(2)))
    row["planes"] = " ".join(f"{l}={n}" for l, n in pairs)

    prepared = stem + "_prepared.kicad_pcb"

    def prep():
        b = pcbnew.LoadBoard(stripped)
        if len(b.GetTracks()) or len(b.Zones()):
            raise RuntimeError("stripped board still has copper")
        pour(b, pairs)
        b.Save(prepared)
        with_project(src_pro, prepared)
        return True

    if step("pour", prep) is None:
        return row
    before = step("drc_before", lambda: drc(prepared, stem + "_prepared_drc.rpt"))
    if before:
        row["kicad_unconnected_before"] = before[0]
        # Violations already on the board before any copper was added: pad-to-pad
        # and similar. They are in the "after" counts too, and are not the router's.
        for k in ("clearance", "shorting_items", "hole_to_hole"):
            row["kicad_" + k + "_before"] = before[1].get(k, 0)

    raw = stem + "_routed_raw.kicad_pcb"
    code, out, secs = run([AUTOLAYOUT, "route", prepared, *flags, "--out", raw], timeout=3600)
    open(stem + "_route.log", "w", encoding="utf-8").write(out)
    row["route_s"] = round(secs, 1)
    m = re.search(r"nets complete (\d+) of (\d+); pads connected (\d+) of (\d+); ([\d.]+) mm of track, (\d+) vias", out)
    if m:
        (row["nets_complete"], row["nets_total"], row["pads_connected"], row["pads_total"],
         row["track_mm"], row["vias"]) = m.groups()
    if not os.path.exists(raw):
        row["status"], row["error"] = "FAIL:route", out.strip()[-300:]
        return row
    with_project(src_pro, raw)

    routed = stem + "_routed.kicad_pcb"

    def refill():
        b = pcbnew.LoadBoard(raw)
        pcbnew.ZONE_FILLER(b).Fill(b.Zones())
        b.Save(routed)
        with_project(src_pro, routed)
        tracks = list(b.GetTracks())
        v = sum(1 for t in tracks if t.GetClass() == "PCB_VIA")
        row["segments"] = len(tracks) - v
        row["vias"] = v
        row["track_mm"] = round(sum(pcbnew.ToMM(t.GetLength()) for t in tracks if t.GetClass() != "PCB_VIA"), 1)
        return True

    if step("refill", refill) is None:
        return row
    after = step("drc_after", lambda: drc(routed, stem + "_routed_drc.rpt"))
    if after:
        unc, kinds = after
        row["kicad_unconnected_after"] = unc
        for k in ("clearance", "shorting_items", "hole_to_hole", "track_dangling", "via_dangling",
                  "track_width", "copper_edge_clearance", "hole_clearance"):
            row["kicad_" + k] = kinds.get(k, 0)
        other = {k: v for k, v in kinds.items() if k not in NON_ROUTING and k not in (
            "unconnected_items", "clearance", "shorting_items", "hole_to_hole", "track_dangling",
            "via_dangling", "track_width", "copper_edge_clearance", "hole_clearance")}
        row["kicad_other_kinds"] = json.dumps(other) if other else ""

    code, out, _ = run([AUTOLAYOUT, "verify", routed, *flags])
    open(stem + "_verify.txt", "w", encoding="utf-8").write(out)
    for key, col in (("unconnected", "verifier_unconnected"), ("clearance", "verifier_clearance"),
                     ("shorting", "verifier_shorting"), ("hole_to_hole", "verifier_hole_to_hole"),
                     ("track_dangling", "verifier_track_dangling"), ("track_width", "verifier_track_width")):
        row[col] = parse_summary(out.splitlines()[0] if out else "", key)

    if freerouting and len(pairs) >= 2:
        fr_dir = os.path.join(work, "fr")
        code, out, secs = run([sys.executable, os.path.join(HERE, "route_check.py"), stripped, fr_dir,
                               pairs[0][1], pairs[1][1], str(FR_PASSES)],
                              timeout=FR_TIMEOUT + 600, env={**os.environ, "FREEROUTING_TIMEOUT": str(FR_TIMEOUT)})
        open(stem + "_freerouting.log", "w", encoding="utf-8").write(out)
        row["freerouting_s"] = round(secs, 1)
        m = re.search(r'"unconnected_after_routing":\s*(\d+)', out)
        if m:
            row["freerouting_unconnected_after"] = int(m.group(1))
    if not row["status"]:
        row["status"] = "ok"
    return row


def write_all(out_dir, rows):
    with open(os.path.join(out_dir, "results.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=COLUMNS)
        w.writeheader()
        w.writerows(rows)
    json.dump(rows, open(os.path.join(out_dir, "results.json"), "w", encoding="utf-8"), indent=1)
    cols = ["board", "status", "nets_complete", "nets_total", "kicad_unconnected_before", "kicad_unconnected_after",
            "kicad_clearance_before", "kicad_clearance", "kicad_shorting_items", "kicad_hole_to_hole", "kicad_track_dangling",
            "verifier_unconnected", "verifier_clearance", "route_s"]
    if any(r["freerouting_unconnected_after"] != "" for r in rows):
        cols += ["freerouting_unconnected_after", "freerouting_s"]
    with open(os.path.join(out_dir, "summary.md"), "w", encoding="utf-8") as f:
        f.write("| " + " | ".join(cols) + " |\n|" + "---|" * len(cols) + "\n")
        for r in rows:
            f.write("| " + " | ".join(str(r[c]) for c in cols) + " |\n")


def main(argv):
    global CORPUS, FR_TIMEOUT, FR_PASSES
    if not argv or argv[0].startswith("--"):
        sys.exit(__doc__)
    out_dir, args = argv[0], argv[1:]
    CORPUS = os.environ.get("AUTOLAYOUT_CORPUS")
    if not CORPUS:
        sys.exit("AUTOLAYOUT_CORPUS is not set")
    if os.path.exists(out_dir):
        sys.exit(f"{out_dir} already exists; refusing to overwrite (use a fresh directory)")
    globs, skip, limit, fr = [], [], None, False
    i = 0
    while i < len(args):
        if args[i] == "--boards":
            i += 1
            while i < len(args) and not args[i].startswith("--"):
                globs.append(args[i])
                i += 1
            continue
        if args[i] == "--skip":
            skip = [s for s in args[i + 1].split(",") if s]
            i += 2
            continue
        if args[i] == "--limit":
            limit = int(args[i + 1])
            i += 2
            continue
        if args[i] == "--fr-timeout":
            FR_TIMEOUT = int(args[i + 1])
            i += 2
            continue
        if args[i] == "--fr-passes":
            FR_PASSES = int(args[i + 1])
            i += 2
            continue
        if args[i] == "--freerouting":
            fr = True
            i += 1
            continue
        sys.exit(f"unknown argument {args[i]}")
    if not globs:
        globs = [f"quilter/{d}/**/*.kicad_pcb" for d in ("fab", "returned", "finish", "routed")]
    boards = []
    for g in globs:
        boards += sorted(glob.glob(os.path.join(CORPUS, g), recursive=True))
    boards = [b for b in dict.fromkeys(boards) if not any(s in os.path.relpath(b, CORPUS).replace("\\", "/") for s in skip)]
    if limit:
        boards = boards[:limit]
    os.makedirs(out_dir)
    rows = []
    for n, b in enumerate(boards, 1):
        rel = os.path.relpath(b, CORPUS).replace("\\", "/")
        print(f"[{n}/{len(boards)}] {rel}", flush=True)
        work = os.path.join(out_dir, re.sub(r"[^A-Za-z0-9_.-]+", "__", rel))
        try:
            row = do_board(b, work, fr)
        except Exception as e:  # never let one board stop the batch
            row = {c: "" for c in COLUMNS}
            row["board"], row["status"], row["error"] = rel, "FAIL:unexpected", str(e)[:300]
        print(f"    {row['status']}  nets {row['nets_complete']}/{row['nets_total']}  "
              f"KiCad unconnected {row['kicad_unconnected_before']} -> {row['kicad_unconnected_after']}  "
              f"clearance {row['kicad_clearance']}  {row['error']}", flush=True)
        rows.append(row)
        write_all(out_dir, rows)
    print(f"done: {len(rows)} board(s); see {out_dir}/summary.md")


if __name__ == "__main__":
    main(sys.argv[1:])
