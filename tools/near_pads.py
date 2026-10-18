"""List the pads within a radius of a point on a board, nearest first.

    python tools/near_pads.py BOARD.kicad_pcb X Y [RADIUS_MM]

Uses `autolayout pads` (absolute pad centres, side, angle). Uses $AUTOLAYOUT or target/release/autolayout.exe.
"""

import math
import os
import subprocess
import sys

EXE = os.environ.get("AUTOLAYOUT", os.path.join("target", "release", "autolayout.exe"))


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    board, x, y = argv[0], float(argv[1]), float(argv[2])
    radius = float(argv[3]) if len(argv) > 3 else 3.0
    out = subprocess.run([EXE, "pads", board], capture_output=True, text=True, encoding="utf-8", errors="replace").stdout
    rows = []
    for line in out.splitlines():
        f = line.split("\t")
        if len(f) < 6:
            continue
        try:
            d = math.hypot(float(f[3]) - x, float(f[4]) - y)
        except ValueError:
            continue
        if d <= radius:
            rows.append((d, f))
    for d, f in sorted(rows):
        print(f"{d:6.2f} mm  {f[0]}.{f[1]}  side {f[2]}  at ({f[3]}, {f[4]})  angle {f[5]}")


if __name__ == "__main__":
    main(sys.argv[1:])
