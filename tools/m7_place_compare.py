"""Placement judged by routability: place a freeplace board with `autolayout
place`, route the result with `autolayout route`, and put the numbers beside
the same measurements of the commercial service's return of the same board.
No KiCad is needed for the table; the headline columns come from our own
router and file verifier, so they are UNVERIFIED until the written boards go
through `tools/m7_check.py` (KiCad DRC), which this script can chain.

    python tools/m7_place_compare.py FREEPLACE.kicad_pcb NETLIST.net RETURNED.kicad_pcb \
        OUT_DIR [--pro PROJECT.kicad_pro] [--plane LAYER=NET ...] [--variant NAME=FLAGS ...]
        [--judge GROUND_NET PLANE_NET]

- FREEPLACE: the staged board (fixed parts on the outline, the rest on a grid).
- NETLIST: the `.net` for that board (boards carry none; it lives in the
  Portcullis tree, not in this repo, and is never committed here).
- RETURNED: the service's placed-and-routed board; its copper is stripped so
  both sides are routed from bare pads by the same router.
- --pro: the `.kicad_pro` whose rules apply to every row (default: the one
  beside FREEPLACE). The router refuses to guess rules.
- --variant NAME=FLAGS: extra `autolayout place` runs, FLAGS comma-separated
  (e.g. `nohalo=--no-halo`). `default` (no flags) is always run.
- --judge: after the table, run `tools/m7_check.py` (KiCad Python) on every
  stripped board so the unconnected count is KiCad's, not ours.

Each row: parts placed, courtyard overlaps and ratsnest from `autolayout grade`,
then nets complete / unconnected / track mm / vias from `autolayout route`.
Same seed and flags give the same bytes (CLAUDE.md rule 4), so rows are
comparable between runs.

STATUS: written 2026-10-13 in a cloud session without the netlist or KiCad;
it has not been run. Fix small things on first use.
"""

import csv
import os
import re
import shutil
import subprocess
import sys

AUTOLAYOUT = os.environ.get("AUTOLAYOUT", os.path.join("target", "release", "autolayout.exe"))
HERE = os.path.dirname(os.path.abspath(__file__))


def run(cmd, timeout=3600):
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def first(pattern, text, cast=float):
    m = re.search(pattern, text)
    return cast(m.group(1)) if m else ""


def measure(label, pcb, pro, planes, out_dir):
    """Strip, route and grade one board; returns (row, stripped path)."""
    stem = os.path.join(out_dir, label)
    stripped = stem + "_stripped.kicad_pcb"
    code, out = run([AUTOLAYOUT, "strip", pcb, stripped])
    if code != 0:
        return {"label": label, "error": "strip failed: " + out.strip()[:200]}, None
    shutil.copyfile(pro, stem + "_stripped.kicad_pro")
    _, grade = run([AUTOLAYOUT, "grade", stripped])
    open(stem + "_grade.txt", "w", encoding="utf-8").write(grade)
    flags = [a for p in planes for a in ("--plane", p)]
    code, rout = run([AUTOLAYOUT, "route", stripped, *flags, "--out", stem + "_routed.kicad_pcb"])
    open(stem + "_route.txt", "w", encoding="utf-8").write(rout)
    row = {
        "label": label,
        "footprints": first(r"footprints\s+(\d+)", grade, int),
        "courtyard_overlaps": first(r"courtyard overlaps\s+(\d+)", grade, int),
        "off_board": first(r"off board\s+(\d+)", grade, int),
        "nets_complete": first(r"nets complete (\d+) of", rout, int),
        "nets_total": first(r"nets complete \d+ of (\d+)", rout, int),
        "verifier_unconnected": first(r"unconnected (\d+), track_dangling", rout, int),
        "clearance": first(r"clearance (\d+), shorting", rout, int),
        "track_mm": first(r"([\d.]+) mm of track", rout),
        "vias": first(r"(\d+) vias,", rout, int),
        # route exits non-zero when anything is unconnected; that is a result.
        "error": "" if "nets complete" in rout else rout.strip()[-200:],
    }
    return row, stem + "_routed.kicad_pcb"


def main(argv):
    pos, planes, variants, judge, pro = [], [], [], None, None
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--plane":
            planes.append(argv[i + 1]); i += 2
        elif a == "--pro":
            pro = argv[i + 1]; i += 2
        elif a == "--variant":
            variants.append(argv[i + 1]); i += 2
        elif a == "--judge":
            judge = (argv[i + 1], argv[i + 2]); i += 3
        else:
            pos.append(a); i += 1
    if len(pos) != 4:
        sys.exit(__doc__)
    freeplace, net, returned, out_dir = pos
    pro = pro or os.path.splitext(freeplace)[0] + ".kicad_pro"
    if not os.path.exists(pro):
        sys.exit(f"no project file {pro}: pass --pro")
    os.makedirs(out_dir, exist_ok=True)
    runs = [("default", [])]
    for v in variants:
        name, _, flags = v.partition("=")
        runs.append((name, [f for f in flags.split(",") if f]))
    rows, stripped = [], []
    for name, flags in runs:
        placed = os.path.join(out_dir, f"placed_{name}.kicad_pcb")
        code, out = run([AUTOLAYOUT, "place", freeplace, net, placed, *flags])
        open(os.path.join(out_dir, f"placed_{name}.txt"), "w", encoding="utf-8").write(out)
        if code != 0:
            rows.append({"label": f"ours:{name}", "error": "place failed: " + out.strip()[-200:]})
            continue
        row, path = measure(f"ours_{name}", placed, pro, planes, out_dir)
        row["label"] = f"ours:{name}"
        rows.append(row)
        stripped.append(path)
    row, path = measure("service", returned, pro, planes, out_dir)
    row["label"] = "service"
    rows.append(row)
    cols = ["label", "footprints", "courtyard_overlaps", "off_board", "nets_complete", "nets_total",
            "verifier_unconnected", "clearance", "track_mm", "vias", "error"]
    with open(os.path.join(out_dir, "place_compare.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=cols)
        w.writeheader()
        w.writerows(rows)
    for r in rows:
        print("  ".join(f"{c}={r.get(c, '')}" for c in cols if r.get(c, "") != ""))
    print("UNVERIFIED: our router and verifier produced these numbers, not KiCad.")
    if judge:
        for label in [f"ours_{n}" for n, _ in runs] + ["service"]:
            s = os.path.join(out_dir, f"{label}_stripped.kicad_pcb")
            if os.path.exists(s):
                code, out = run([sys.executable, os.path.join(HERE, "m7_check.py"), s,
                                 os.path.join(out_dir, "judge_" + label), judge[0], judge[1]])
                print(f"--- KiCad judge: {label} (exit {code})\n{out[-1500:]}")


if __name__ == "__main__":
    main(sys.argv[1:])
