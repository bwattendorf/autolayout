"""Judge autolayout's own router with KiCad: pour planes on a stripped board,
route it with `autolayout route`, refill, and let `kicad-cli pcb drc` count
clearance violations and unconnected items (CLAUDE.md rule 3: the tool that
made the board does not grade it).

Run under KiCad's bundled Python (needs pcbnew), after `cargo build --release`:

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/m7_check.py \
        STRIPPED.kicad_pcb OUT_DIR GROUND_NET PLANE_NET

STRIPPED comes from `autolayout strip` and needs its `.kicad_pro` beside it:
the router refuses to guess design rules (M7.0). GROUND_NET is poured on
In1.Cu and PLANE_NET on In2.Cu, the same nets for every board, chosen
structurally (`autolayout groups`), never by name here. The router is told
the same planes. Prints one JSON object; the numbers that matter are
`drc.unconnected_after` and the violation kinds.

STATUS: written 2026-10-13 in a cloud session with no KiCad; it has not been
run. First run on the Windows machine may need small fixes.
"""

import json
import os
import re
import shutil
import subprocess
import sys

import pcbnew

KICAD_CLI = os.environ.get("KICAD_CLI", r"C:/Program Files/KiCad/9.0/bin/kicad-cli.exe")
AUTOLAYOUT = os.environ.get("AUTOLAYOUT", os.path.join("target", "release", "autolayout.exe"))


def drc(pcb, report):
    subprocess.run([KICAD_CLI, "pcb", "drc", "-o", report, pcb], capture_output=True, text=True)
    text = open(report, encoding="utf-8", errors="replace").read()
    unconnected = re.search(r"Found (\d+) unconnected", text)
    kinds = {}
    for m in re.finditer(r"^\[([a-z_]+)\]", text, re.M):
        kinds[m.group(1)] = kinds.get(m.group(1), 0) + 1
    return (int(unconnected.group(1)) if unconnected else None), kinds


def pour(board, ground, plane, inset_mm=0.5):
    plan = [(pcbnew.In1_Cu, ground), (pcbnew.In2_Cu, plane)]
    bb = board.GetBoardEdgesBoundingBox()
    x0, y0 = pcbnew.ToMM(bb.GetLeft()) + inset_mm, pcbnew.ToMM(bb.GetTop()) + inset_mm
    x1, y1 = pcbnew.ToMM(bb.GetRight()) - inset_mm, pcbnew.ToMM(bb.GetBottom()) - inset_mm
    for layer, name in plan:
        net = board.FindNet(name)
        if net is None:
            raise SystemExit(f"net {name!r} not on this board")
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


def with_project(src, dst_pcb):
    shutil.copyfile(os.path.splitext(src)[0] + ".kicad_pro", os.path.splitext(dst_pcb)[0] + ".kicad_pro")


def main(src, out_dir, ground, plane):
    os.makedirs(out_dir, exist_ok=True)
    stem = os.path.join(out_dir, os.path.splitext(os.path.basename(src))[0])
    board = pcbnew.LoadBoard(src)
    if len(board.GetTracks()) or len(board.Zones()):
        raise SystemExit("input still has copper; run `autolayout strip IN OUT` first")
    pour(board, ground, plane)
    prepared = stem + "_prepared.kicad_pcb"
    board.Save(prepared)
    with_project(src, prepared)

    raw = stem + "_routed_raw.kicad_pcb"
    for p in (raw,):
        if os.path.exists(p):
            os.remove(p)  # autolayout refuses to overwrite, by design
    run = subprocess.run([AUTOLAYOUT, "route", prepared, "--plane", f"In1.Cu={ground}",
                          "--plane", f"In2.Cu={plane}", "--out", raw], capture_output=True, text=True)
    open(stem + "_route.log", "w", encoding="utf-8").write(run.stdout + run.stderr)
    if run.returncode not in (0, 1) or not os.path.exists(raw):
        raise SystemExit(f"autolayout route failed (exit {run.returncode}); see {stem}_route.log")

    with_project(src, raw)
    routed_board = pcbnew.LoadBoard(raw)
    pcbnew.ZONE_FILLER(routed_board).Fill(routed_board.Zones())
    routed = stem + "_routed.kicad_pcb"
    routed_board.Save(routed)
    with_project(src, routed)

    before, _ = drc(prepared, stem + "_prepared_drc.rpt")
    after, kinds = drc(routed, stem + "_routed_drc.rpt")
    tracks = list(routed_board.GetTracks())
    vias = sum(1 for t in tracks if t.GetClass() == "PCB_VIA")
    length = sum(pcbnew.ToMM(t.GetLength()) for t in tracks if t.GetClass() != "PCB_VIA")
    print(json.dumps({
        "input": src,
        "planes": {"In1.Cu": ground, "In2.Cu": plane},
        "router_summary": [l for l in run.stdout.splitlines() if "route (UNVERIFIED" in l or "iteration" in l],
        "segments": len(tracks) - vias,
        "vias": vias,
        "track_mm": round(length, 1),
        "drc": {"unconnected_before_routing": before, "unconnected_after": after, "violations_by_kind": kinds},
        "routed_board": routed,
    }, indent=2))


if __name__ == "__main__":
    if len(sys.argv) != 5:
        sys.exit(__doc__)
    main(*sys.argv[1:5])
