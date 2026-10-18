"""Try placer settings on one freeplace board and route each result quickly.

    python tools/halo_try.py BOARD_ID OUT_DIR [--scales 1 1.5 2 3] [--extra "--grade"] [--iters 40]

For each scale: `autolayout place` (with --halo-scale S and --extra flags), then `autolayout route
--quick --iters N` on the placed board with the planes `autolayout planes` finds. Prints the router's
own verifier unconnected count (NOT KiCad's; use tools/place_sweep.py for the KiCad verdict).
Uses the release binary at $AUTOLAYOUT or target_dev/release/autolayout.exe. Inputs are never touched.
"""

import glob
import os
import re
import subprocess
import sys

CORPUS = os.environ.get("AUTOLAYOUT_CORPUS", "C:/Portcullis/controller/boards")
EXE = os.environ.get("AUTOLAYOUT", os.path.join("target_dev", "release", "autolayout.exe"))


def run(cmd, timeout=3600):
    p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    bid, out_dir = argv[0], argv[1]
    scales, extra, iters = ["1"], [], "40"
    i = 2
    while i < len(argv):
        if argv[i] == "--scales":
            scales = []
            i += 1
            while i < len(argv) and not argv[i].startswith("--"):
                scales.append(argv[i]); i += 1
        elif argv[i] == "--extra":
            extra = argv[i + 1].split(); i += 2
        elif argv[i] == "--iters":
            iters = argv[i + 1]; i += 2
        else:
            sys.exit("unknown " + argv[i])
    src = os.path.join(CORPUS, "quilter", "freeplace", bid, bid + ".kicad_pcb")
    pro = os.path.splitext(src)[0] + ".kicad_pro"
    net = None
    for d in sorted(glob.glob(os.path.join(CORPUS, bid + "-*"))):
        nets = sorted(glob.glob(os.path.join(d, "hardware", "kicad", "*.net")))
        if len(nets) == 1:
            net = nets[0]
    if not (os.path.exists(src) and net):
        sys.exit("no freeplace board or netlist for " + bid)
    os.makedirs(out_dir, exist_ok=True)
    for s in scales:
        tag = f"s{s}" + ("_" + "".join(c for c in " ".join(extra) if c.isalnum()) if extra else "")
        placed = os.path.join(out_dir, f"{bid}_{tag}.kicad_pcb")
        if os.path.exists(placed):
            os.remove(placed)
        code, out = run([EXE, "place", src, net, placed, "--halo-scale", s, *extra])
        if code != 0 or not os.path.exists(placed):
            print(f"scale {s}: place failed: {out.strip()[-200:]}")
            continue
        with open(placed.replace(".kicad_pcb", ".kicad_pro"), "w", encoding="utf-8") as f:
            f.write(open(pro, encoding="utf-8").read())
        code, out = run([EXE, "planes", placed])
        flags = []
        for line in out.splitlines():
            if line.startswith("FLAGS\t"):
                flags = [x for x in line.split("\t")[1:] if x]
        routed = placed.replace(".kicad_pcb", "_routed.kicad_pcb")
        if os.path.exists(routed):
            os.remove(routed)
        code, out = run([EXE, "route", placed, *flags, "--quick", "--iters", iters, "--out", routed])
        m = re.search(r"verifier unconnected (\d+)", out)
        n = re.search(r"nets complete (\d+) of (\d+)", out)
        print(f"{bid} halo-scale {s} {' '.join(extra)}: router unconnected {m.group(1) if m else '?'}, "
              f"nets complete {n.group(1) + '/' + n.group(2) if n else '?'}", flush=True)


if __name__ == "__main__":
    main(sys.argv[1:])
