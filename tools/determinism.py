"""Determinism check: run the same command twice and compare the output files byte for byte.

    python tools/determinism.py BOARD_ID OUT_DIR

Places the freeplace board with `autolayout place` twice, then routes the first placement twice (full
default pipeline, with its parallel portfolio threads) under the fab rules, and compares the placed
files and the routed files. Prints one line per comparison. CLAUDE.md rule 4: same input + same
options + same seed => the same output bytes. Uses $AUTOLAYOUT or target_dev/release/autolayout.exe.
"""

import filecmp
import glob
import os
import shutil
import subprocess
import sys

CORPUS = os.environ.get("AUTOLAYOUT_CORPUS", "C:/Portcullis/controller/boards")
EXE = os.environ.get("AUTOLAYOUT", os.path.join("target_dev", "release", "autolayout.exe"))


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    bid, out = argv[0], argv[1]
    os.makedirs(out, exist_ok=False)
    src = os.path.join(CORPUS, "quilter", "freeplace", bid, bid + ".kicad_pcb")
    net = sorted(glob.glob(os.path.join(CORPUS, bid + "-*", "hardware", "kicad", "*.net")))[0]
    pro = next(p for p in (os.path.join(CORPUS, "quilter", d, bid, bid + ".kicad_pro") for d in ("fab", "finish")) if os.path.exists(p)) \
        if any(os.path.exists(os.path.join(CORPUS, "quilter", d, bid, bid + ".kicad_pro")) for d in ("fab", "finish")) \
        else sorted(glob.glob(os.path.join(CORPUS, "quilter", "returned", bid, "3p5mil-candidate*", bid + ".kicad_pro")))[0]
    placed = [os.path.join(out, f"placed{k}.kicad_pcb") for k in (1, 2)]
    for p in placed:
        code, text = run([EXE, "place", src, net, p])
        if code != 0:
            sys.exit("place failed: " + text[-300:])
    print("placed twice:", "identical" if filecmp.cmp(placed[0], placed[1], shallow=False) else "DIFFERENT")
    work = []
    for k in (1, 2):
        w = os.path.join(out, f"route{k}.kicad_pcb")
        shutil.copyfile(placed[0], w)
        shutil.copyfile(pro, os.path.splitext(w)[0] + ".kicad_pro")
        code, text = run([EXE, "planes", w])
        flags = []
        for line in text.splitlines():
            if line.startswith("FLAGS\t"):
                flags = [x for x in line.split("\t")[1:] if x]
        r = os.path.join(out, f"routed{k}.kicad_pcb")
        code, text = run([EXE, "route", w, *flags, "--out", r])
        if not os.path.exists(r):
            sys.exit("route failed: " + text[-300:])
        work.append(r)
    print("routed twice:", "identical" if filecmp.cmp(work[0], work[1], shallow=False) else "DIFFERENT")


if __name__ == "__main__":
    main(sys.argv[1:])
