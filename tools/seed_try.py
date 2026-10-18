"""Route one board with several --seed values and print the router's unconnected count for each.

    python tools/seed_try.py PLACED.kicad_pcb [--seeds 0 1 2 3 4 5 6 7] [--quick] [--iters 40]

PLACED needs its .kicad_pro beside it. Planes come from `autolayout route --auto-planes`. Prints one line
per seed. The count is the router's own verifier (not KiCad's). Uses $AUTOLAYOUT or
target_dev/release/autolayout.exe.
"""

import os
import re
import subprocess
import sys

EXE = os.environ.get("AUTOLAYOUT", os.path.join("target_dev", "release", "autolayout.exe"))


def main(argv):
    if not argv:
        sys.exit(__doc__)
    board = argv[0]
    seeds = ["0", "1", "2", "3", "4", "5", "6", "7"]
    if "--seeds" in argv:
        i = argv.index("--seeds") + 1
        seeds = []
        while i < len(argv) and not argv[i].startswith("--"):
            seeds.append(argv[i]); i += 1
    extra = (["--quick"] if "--quick" in argv else []) + (["--iters", argv[argv.index("--iters") + 1]] if "--iters" in argv else [])
    for s in seeds:
        out = os.path.splitext(board)[0] + f"_seed{s}.kicad_pcb"
        if os.path.exists(out):
            os.remove(out)
        p = subprocess.run([EXE, "route", board, "--auto-planes", "--seed", s, *extra, "--out", out],
                           capture_output=True, text=True, encoding="utf-8", errors="replace")
        text = p.stdout + p.stderr
        best = re.findall(r"verifier unconnected (\d+)", text)
        n = re.search(r"nets complete (\d+) of (\d+); pads", text)
        print(f"seed {s}: attempts' unconnected {best}, chosen nets complete {n.group(1) + '/' + n.group(2) if n else '?'}", flush=True)


if __name__ == "__main__":
    main(sys.argv[1:])
