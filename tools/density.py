"""Where do the parts sit? A coarse occupancy map of a board's footprint centres within its outline.

    python tools/density.py BOARD.kicad_pcb [BOARD2.kicad_pcb ...] [--cells 10]

Prints, per board: the outline box, the number of parts, and a grid of part counts per cell (rows
top to bottom), plus how many cells hold no part. Text only; reads the file, never writes.
"""

import re
import sys


def parts(text):
    out = []
    for m in re.finditer(r'\(footprint\s+"[^"]*"\s*(?:\(locked yes\)\s*)?\(layer "([^"]+)"\)(?:.|\n)*?\(at ([-\d.]+) ([-\d.]+)', text):
        out.append((float(m.group(2)), float(m.group(3)), m.group(1)))
    return out


def outline(text):
    xs, ys = [], []
    for m in re.finditer(r'\(gr_(?:line|rect)\s*\(start ([-\d.]+) ([-\d.]+)\)\s*\(end ([-\d.]+) ([-\d.]+)\)(?:.|\n)*?"Edge\.Cuts"', text):
        xs += [float(m.group(1)), float(m.group(3))]
        ys += [float(m.group(2)), float(m.group(4))]
    return (min(xs), min(ys), max(xs), max(ys)) if xs else None


def main(argv):
    cells = 10
    paths = []
    i = 0
    while i < len(argv):
        if argv[i] == "--cells":
            cells = int(argv[i + 1]); i += 2
        else:
            paths.append(argv[i]); i += 1
    for p in paths:
        text = open(p, encoding="utf-8").read()
        box = outline(text)
        ps = parts(text)
        if not box or not ps:
            print(p, "no outline or no parts")
            continue
        x0, y0, x1, y1 = box
        grid = [[0] * cells for _ in range(cells)]
        for x, y, _ in ps:
            cx = min(cells - 1, max(0, int((x - x0) / (x1 - x0) * cells)))
            cy = min(cells - 1, max(0, int((y - y0) / (y1 - y0) * cells)))
            grid[cy][cx] += 1
        empty = sum(1 for r in grid for c in r if c == 0)
        print(f"{p}\n  outline {x1 - x0:.0f} x {y1 - y0:.0f} mm, {len(ps)} parts, {empty} of {cells * cells} cells empty")
        for r in grid:
            print("  " + " ".join(f"{c:3d}" if c else "  ." for c in r))


if __name__ == "__main__":
    main(sys.argv[1:])
