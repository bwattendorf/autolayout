"""Routability feedback for placement: place, route, give the parts around stuck regions more room, repeat.

    python tools/autoplace.py BOARD_ID OUT_DIR [--rounds 8] [--step 1.6] [--cap 6] [--iters 40]
        [--place-flags "--grade"] [--patience 3]

Round 0 places the freeplace board `BOARD_ID` (netlist found beside the Portcullis tree) with the given
`autolayout place` flags. Each round then routes the placed board with `autolayout route --quick
--explain` and reads the `STUCK x y REF,REF,...` lines (one per pad whose region the router could not
leave: the parts with a pad within 3 mm of it are listed). Those parts' escape halos are multiplied by
--step (up to --cap) through `autolayout place --halo-boost FILE`, and the board is placed again from the
input. The best placement by the router's own unconnected count is kept as OUT_DIR/best.kicad_pcb.
Stops at 0 unconnected, after --rounds, or after --patience rounds without improvement. The count is
the router's verifier's, not KiCad's: judge the kept board with tools/place_sweep.py or m7_batch.
Everything is deterministic; inputs are never touched. Uses $AUTOLAYOUT or target_dev/release/autolayout.exe.
"""

import glob
import os
import re
import shutil
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
    rounds, step, cap, iters, place_flags, patience = 8, 1.6, 6.0, "40", [], 3
    i = 2
    while i < len(argv):
        a = argv[i]
        if a == "--rounds":
            rounds = int(argv[i + 1]); i += 2
        elif a == "--step":
            step = float(argv[i + 1]); i += 2
        elif a == "--cap":
            cap = float(argv[i + 1]); i += 2
        elif a == "--iters":
            iters = argv[i + 1]; i += 2
        elif a == "--place-flags":
            place_flags = argv[i + 1].split(); i += 2
        elif a == "--patience":
            patience = int(argv[i + 1]); i += 2
        else:
            sys.exit("unknown " + a)
    src = os.path.join(CORPUS, "quilter", "freeplace", bid, bid + ".kicad_pcb")
    pro = os.path.splitext(src)[0] + ".kicad_pro"
    net = None
    for d in sorted(glob.glob(os.path.join(CORPUS, bid + "-*"))):
        nets = sorted(glob.glob(os.path.join(d, "hardware", "kicad", "*.net")))
        if len(nets) == 1:
            net = nets[0]
    if not (os.path.exists(src) and os.path.exists(pro) and net):
        sys.exit("no freeplace board, project or netlist for " + bid)
    os.makedirs(out_dir, exist_ok=False)
    boost = {}
    best = None
    stale = 0
    for r in range(rounds + 1):
        placed = os.path.join(out_dir, f"{bid}_r{r}.kicad_pcb")
        cmd = [EXE, "place", src, net, placed, *place_flags]
        if boost:
            bf = os.path.join(out_dir, f"boost_r{r}.txt")
            with open(bf, "w", encoding="utf-8") as f:
                for ref, v in sorted(boost.items()):
                    f.write(f"{ref} {v:.3f}\n")
            cmd += ["--halo-boost", bf]
        code, out = run(cmd)
        if code != 0 or not os.path.exists(placed):
            print(f"round {r}: place failed: {out.strip()[-200:]}")
            break
        shutil.copyfile(pro, os.path.splitext(placed)[0] + ".kicad_pro")
        code, out = run([EXE, "planes", placed])
        flags = []
        for line in out.splitlines():
            if line.startswith("FLAGS\t"):
                flags = [x for x in line.split("\t")[1:] if x]
        routed = placed.replace(".kicad_pcb", "_routed.kicad_pcb")
        code, out = run([EXE, "route", placed, *flags, "--quick", "--iters", iters, "--explain", "--out", routed])
        m = re.search(r"verifier unconnected (\d+)", out)
        n = re.search(r"nets complete (\d+) of (\d+)", out)
        unconnected = int(m.group(1)) if m else 10**9
        stuck = [l.split("\t") for l in out.splitlines() if l.startswith("STUCK\t")]
        print(f"round {r}: router unconnected {unconnected}, nets {n.group(1) + '/' + n.group(2) if n else '?'}, "
              f"{len(stuck)} stuck region(s), {len(boost)} part(s) boosted", flush=True)
        if best is None or unconnected < best[0]:
            best = (unconnected, placed, r)
            stale = 0
            shutil.copyfile(placed, os.path.join(out_dir, "best.kicad_pcb"))
            shutil.copyfile(os.path.splitext(placed)[0] + ".kicad_pro", os.path.join(out_dir, "best.kicad_pro"))
        else:
            stale += 1
        if unconnected == 0 or stale >= patience:
            break
        added = 0
        for parts in stuck:
            for ref in (parts[3].split(",") if len(parts) > 3 and parts[3] else []):
                new = min(cap, boost.get(ref, 1.0) * step)
                if new > boost.get(ref, 1.0):
                    boost[ref] = new
                    added += 1
        if not added:
            print("nothing left to boost; stopping")
            break
    if best:
        print(f"best: round {best[2]}, router unconnected {best[0]}  ({os.path.join(out_dir, 'best.kicad_pcb')})")


if __name__ == "__main__":
    main(sys.argv[1:])
