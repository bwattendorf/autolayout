"""List the keep-out zones of a board that come within a radius of a point.

    python tools/keepouts.py BOARD.kicad_pcb X Y [RADIUS_MM]

For each keep-out zone: its layer(s), what it forbids (tracks/vias/pads/copperpour/footprints), its
bounding box, and the distance from the point to that box. Reads the file only.
"""

import re
import sys


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    text = open(argv[0], encoding="utf-8").read()
    x, y = float(argv[1]), float(argv[2])
    radius = float(argv[3]) if len(argv) > 3 else 3.0
    found = 0
    for m in re.finditer(r"\(zone\b", text):
        start = m.start()
        depth, i = 0, start
        while i < len(text):
            c = text[i]
            if c == "(":
                depth += 1
            elif c == ")":
                depth -= 1
                if depth == 0:
                    break
            i += 1
        block = text[start:i + 1]
        ko = re.search(r"\(keepout\s*((?:\([^()]*\)\s*)*)\)", block)
        if not ko:
            continue
        pts = [(float(a), float(b)) for a, b in re.findall(r"\(xy ([-\d.]+) ([-\d.]+)\)", block)]
        if not pts:
            continue
        x0, x1 = min(p[0] for p in pts), max(p[0] for p in pts)
        y0, y1 = min(p[1] for p in pts), max(p[1] for p in pts)
        dx = max(x0 - x, 0, x - x1)
        dy = max(y0 - y, 0, y - y1)
        d = (dx * dx + dy * dy) ** 0.5
        if d <= radius:
            layers = re.findall(r'\(layers? ([^)]*)\)', block) or re.findall(r'\(layer "([^"]+)"\)', block)
            print(f"{d:5.2f} mm  box ({x0:.2f},{y0:.2f})-({x1:.2f},{y1:.2f})  forbids {ko.group(1).split()}  layers {layers[:2]}")
            found += 1
    print(f"{found} keep-out zone(s) within {radius} mm of ({x}, {y})")


if __name__ == "__main__":
    main(sys.argv[1:])
