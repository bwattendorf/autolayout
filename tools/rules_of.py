"""Print the default net class and minimum rules of one or more .kicad_pro files.

    python tools/rules_of.py FILE.kicad_pro [FILE2.kicad_pro ...]
"""

import json
import sys


def main(argv):
    for p in argv:
        d = json.load(open(p, encoding="utf-8"))
        rules = d.get("board", {}).get("design_settings", {}).get("rules", {})
        classes = d.get("net_settings", {}).get("classes", [])
        c = next((c for c in classes if c.get("name") == "Default"), classes[0] if classes else {})
        print(p)
        print(f"  default class: track {c.get('track_width')}, clearance {c.get('clearance')}, "
              f"via {c.get('via_diameter')}/{c.get('via_drill')}")
        print(f"  board minimums: track {rules.get('min_track_width')}, clearance {rules.get('min_clearance')}, "
              f"via {rules.get('min_via_diameter')}, hole {rules.get('min_through_hole_diameter')}")


if __name__ == "__main__":
    main(sys.argv[1:])
