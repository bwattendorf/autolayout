"""Write a JLCPCB-conformant variant of a KiCad project file (design rules only).

    python tools/jlc_rules.py SRC.kicad_pro DST.kicad_pro [--via 0.25/0.15 | --via 0.45/0.20]

JLCPCB 4-layer capabilities, https://jlcpcb.com/capabilities/pcb-capabilities (read 2026-10-18):
  trace width / spacing 0.09 / 0.09 mm; via hole 0.15 mm, via pad 0.25 mm (the 0.15-0.25 sizes are an extra-cost
  "via size option" chosen when ordering; 0.20 mm holes need pads of 0.45 mm or more at standard cost);
  hole to hole 0.20 mm (via) / 0.45 mm (pad); copper to board edge 0.20 mm (routed); annular ring (PTH) 0.15 mm.
Every net class keeps any value that is already larger than JLCPCB's minimum and is raised to the minimum
otherwise; vias are set to --via (default 0.25/0.15); the board minimums follow. Only the rules change.
Check the live page before ordering: capabilities change and the DFM check at order time has the last word.
"""

import json
import sys

TRACK = 0.09
CLEARANCE = 0.09
HOLE_TO_HOLE = 0.20
EDGE = 0.20


def convert(src, dst, via_d, via_drill):
    d = json.load(open(src, encoding="utf-8"))
    for c in d.get("net_settings", {}).get("classes", []):
        c["track_width"] = max(float(c.get("track_width") or 0), TRACK)
        c["clearance"] = max(float(c.get("clearance") or 0), CLEARANCE)
        c["via_diameter"] = via_d
        c["via_drill"] = via_drill
    rules = d.setdefault("board", {}).setdefault("design_settings", {}).setdefault("rules", {})
    rules["min_track_width"] = max(float(rules.get("min_track_width") or 0), TRACK)
    rules["min_clearance"] = max(float(rules.get("min_clearance") or 0), CLEARANCE)
    rules["min_via_diameter"] = via_d
    rules["min_through_hole_diameter"] = min(via_drill, float(rules.get("min_through_hole_diameter") or via_drill))
    rules["min_hole_to_hole"] = max(float(rules.get("min_hole_to_hole") or 0), HOLE_TO_HOLE)
    rules["min_copper_edge_clearance"] = max(float(rules.get("min_copper_edge_clearance") or 0), EDGE)
    json.dump(d, open(dst, "w", encoding="utf-8"), indent=2)
    return d


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    via = argv[argv.index("--via") + 1] if "--via" in argv else "0.25/0.15"
    d, drill = (float(x) for x in via.split("/"))
    convert(argv[0], argv[1], d, drill)
    print(f"wrote {argv[1]}: track/clearance >= {TRACK}/{CLEARANCE}, via {d}/{drill}, hole-to-hole >= {HOLE_TO_HOLE}")


if __name__ == "__main__":
    main(sys.argv[1:])
