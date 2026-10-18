"""Make a one-board corpus with the unsourceable capacitors swapped, ready for place_sweep.

    python tools/cap_fix.py BOARD_ID NETLIST_DIR OUT_CORPUS REF:LIB|THT ...

Copies quilter/freeplace/<id>/<id>.kicad_pro, the netlist directory and the returned-candidate project files
from $AUTOLAYOUT_CORPUS (default C:/Portcullis/controller/boards) into OUT_CORPUS, then applies
tools/swap_to_tht2.py once per `REF:SPEC`: SPEC `THT` is the 5.08 mm-pitch 14.5 mm through-hole supercap
footprint; anything else is a Capacitor_SMD footprint file name from the KiCad library, e.g. `CP_Elec_16x17.5`.
The input board is not modified. Then run:
    AUTOLAYOUT_CORPUS=OUT_CORPUS python tools/place_sweep.py OUT --rules jlc-std --boards ID --check
"""

import glob
import os
import shutil
import subprocess
import sys

LIB = "C:/Program Files/KiCad/10.0/share/kicad/footprints/Capacitor_SMD.pretty/%s.kicad_mod"


def main(argv):
    if len(argv) < 4:
        sys.exit(__doc__)
    bid, net_dir, out = argv[0], argv[1], argv[2]
    corpus = os.environ.get("AUTOLAYOUT_CORPUS", "C:/Portcullis/controller/boards")
    fp = os.path.join(out, "quilter", "freeplace", bid)
    os.makedirs(fp, exist_ok=True)
    shutil.copy(os.path.join(corpus, "quilter", "freeplace", bid, bid + ".kicad_pro"), fp)
    nd = os.path.join(out, net_dir, "hardware", "kicad")
    os.makedirs(nd, exist_ok=True)
    for n in glob.glob(os.path.join(corpus, net_dir, "hardware", "kicad", "*.net")):
        shutil.copy(n, nd)
    for d in glob.glob(os.path.join(corpus, "quilter", "returned", bid, "3p5mil-candidate*")):
        dd = os.path.join(out, "quilter", "returned", bid, os.path.basename(d))
        os.makedirs(dd, exist_ok=True)
        shutil.copy(os.path.join(d, bid + ".kicad_pro"), dd)
    cur = os.path.join(corpus, "quilter", "freeplace", bid, bid + ".kicad_pcb")
    dst = os.path.join(fp, bid + ".kicad_pcb")
    for i, spec in enumerate(argv[3:]):
        ref, kind = spec.split(":", 1)
        last = i == len(argv) - 4
        nxt = dst if last else os.path.join(out, f"{bid}_step{i}.kicad_pcb")
        extra = ["--pitch", "5.08", "--body-dia", "14.5"] if kind == "THT" else ["--lib", LIB % kind]
        r = subprocess.run([sys.executable, os.path.join(os.path.dirname(__file__), "swap_to_tht2.py"), cur, nxt, ref, "--unlock", *extra],
                           capture_output=True, text=True)
        print((r.stdout + r.stderr).strip())
        if r.returncode:
            sys.exit(1)
        cur = nxt


if __name__ == "__main__":
    main(sys.argv[1:])
