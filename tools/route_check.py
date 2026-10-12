"""Routability of a placement: strip copper, pour identical planes, route with
Freerouting, import, and let KiCad's DRC count what is still unconnected.

Run under KiCad's bundled Python (needs pcbnew):

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/route_check.py \
        IN.kicad_pcb OUT_DIR GROUND_NET PLANE_NET [PASSES]

Why each step is shaped this way:

- Copper is STRIPPED first (tracks, vias, zones), by `autolayout strip`
  before this script runs. A returned board already carries its router's
  copper; comparing placements means every board starts from bare pads.
- Planes are poured BEFORE the DSN export. KiCad writes `(plane ...)` only for
  zones that exist at export time; unpoured, the router routes GND as traces.
  Measured on Portcullis m1 (layout_import.add_zones docstring): 49 vs 59
  unrouted, same board and passes.
- The plan is fixed and identical for every board: GROUND_NET on F.Cu, In1.Cu
  and B.Cu, PLANE_NET on In2.Cu, inset 0.5 mm. The nets are passed in, chosen
  structurally by `autolayout groups` (ground = widest supply net), never by
  name inside this script.
- The verdict is KiCad's (`kicad-cli pcb drc`), not Freerouting's own count:
  the router grading itself is the thing autolayout's rule 3 forbids.

Freerouting is GPL; it is RUN here as an external program and none of its
code is in this repository. Uses only the public pcbnew API.
"""

import json
import os
import re
import subprocess
import sys
import time

import pcbnew

JAVA = os.environ.get("JAVA", r"C:/Program Files/Eclipse Adoptium/jre-25.0.4.7-hotspot/bin/java.exe")
JAR = os.environ.get("FREEROUTING_JAR", r"C:/hamradio/tools/freerouting/freerouting-2.3.0.jar")
KICAD_CLI = os.environ.get("KICAD_CLI", r"C:/Program Files/KiCad/9.0/bin/kicad-cli.exe")


def require_bare(board):
    """The input must already be stripped (`autolayout strip`): KiCad 9.0's
    SWIG binding breaks every later object after board.Remove(), so this
    script never removes anything. Refuse a board that still has copper."""
    tracks, zones = len(board.GetTracks()), len(board.Zones())
    if tracks or zones:
        raise SystemExit(f"input still has {tracks} track/via item(s) and {zones} zone(s); "
                         "run `autolayout strip IN OUT` first")


def pour(board, ground, plane, inset_mm=0.5):
    # board.FindNet, not GetNetsByName(): in KiCad 9.0's SWIG binding the
    # latter raises AttributeError ('SwigPyObject' has no 'NetsByName') once
    # items have been removed from the board, which strip_copper just did.
    plan = [(pcbnew.F_Cu, ground), (pcbnew.In1_Cu, ground), (pcbnew.In2_Cu, plane), (pcbnew.B_Cu, ground)]
    bb = board.GetBoardEdgesBoundingBox()
    x0, y0 = pcbnew.ToMM(bb.GetLeft()) + inset_mm, pcbnew.ToMM(bb.GetTop()) + inset_mm
    x1, y1 = pcbnew.ToMM(bb.GetRight()) - inset_mm, pcbnew.ToMM(bb.GetBottom()) - inset_mm
    made = 0
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
        made += 1
    pcbnew.ZONE_FILLER(board).Fill(board.Zones())
    return made


def drc(pcb, report):
    subprocess.run([KICAD_CLI, "pcb", "drc", "-o", report, pcb], capture_output=True, text=True)
    text = open(report, encoding="utf-8", errors="replace").read()
    unconnected = re.search(r"Found (\d+) unconnected", text)
    kinds = {}
    for m in re.finditer(r"^\[([a-z_]+)\]", text, re.M):
        kinds[m.group(1)] = kinds.get(m.group(1), 0) + 1
    return (int(unconnected.group(1)) if unconnected else None), kinds


def main(src, out_dir, ground, plane, passes):
    os.makedirs(out_dir, exist_ok=True)
    stem = os.path.join(out_dir, os.path.splitext(os.path.basename(src))[0])
    board = pcbnew.LoadBoard(src)
    require_bare(board)
    planes = pour(board, ground, plane)
    prepared = stem + "_prepared.kicad_pcb"
    board.Save(prepared)

    dsn, ses = stem + ".dsn", stem + ".ses"
    if not pcbnew.ExportSpecctraDSN(board, dsn):
        raise SystemExit("DSN export failed")

    t0 = time.time()
    log = open(stem + "_freerouting.log", "w", encoding="utf-8")
    # HEADLESS, both ways. Without these Freerouting opens a window on the
    # user's screen, and on the first comparison run (2026-10-12) that window
    # threw a NullPointerException from its plane-drawing code
    # (ConductionArea.draw on the AWT EventDispatchThread). The router thread
    # was unaffected, but a tool must not pop errors at the user.
    proc = subprocess.run([JAVA, "-Djava.awt.headless=true", "-jar", JAR, "--gui.enabled=false",
                           "-de", dsn, "-do", ses, "-mp", str(passes), "-mt", "1"],
                          stdout=log, stderr=subprocess.STDOUT)
    log.close()
    route_s = time.time() - t0
    if not os.path.exists(ses):
        raise SystemExit(f"Freerouting wrote no .ses (exit {proc.returncode}); see {stem}_freerouting.log")

    routed = pcbnew.LoadBoard(prepared)
    if not pcbnew.ImportSpecctraSES(routed, ses):
        raise SystemExit("SES import failed")
    pcbnew.ZONE_FILLER(routed).Fill(routed.Zones())
    out = stem + "_routed.kicad_pcb"
    routed.Save(out)

    unconnected_before, _ = drc(prepared, stem + "_prepared_drc.rpt")
    unconnected, kinds = drc(out, stem + "_routed_drc.rpt")
    tracks = [t for t in routed.GetTracks()]
    vias = sum(1 for t in tracks if t.GetClass() == "PCB_VIA")
    length = sum(pcbnew.ToMM(t.GetLength()) for t in tracks if t.GetClass() != "PCB_VIA")
    result = {
        "input": src,
        "planes": planes,
        "passes": passes,
        "route_seconds": round(route_s, 1),
        "unconnected_before_routing": unconnected_before,
        "unconnected_after_routing": unconnected,
        "segments": len(tracks) - vias,
        "vias": vias,
        "track_mm": round(length, 1),
        "drc": kinds,
        "routed_board": out,
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    if len(sys.argv) not in (5, 6):
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5]) if len(sys.argv) == 6 else 100)
