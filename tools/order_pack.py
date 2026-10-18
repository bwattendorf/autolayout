"""Write the JLCPCB order files for one placed+routed board.

    python tools/order_pack.py BOARD_ID SWEEP_DIR OUT_DIR [--set MPN_OR_COMMENT=LCSC ...]

SWEEP_DIR is a place_sweep output directory (uses SWEEP_DIR/best/<id>.kicad_pcb). Writes into OUT_DIR:
Gerbers + drill + zip (tools/fab_package.py), the SMD placement file in JLCPCB columns (kicad-cli pos +
tools/jlc_cpl.py), the order BOM and the hand-source list (tools/order_bom.py). Prints one summary line per step.
Through-hole parts are not in the placement file (hand-solder or order assembly separately).
"""

import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
CLI = os.environ.get("KICAD_CLI", "C:/Program Files/KiCad/10.0/bin/kicad-cli.exe")


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    print((p.stdout + p.stderr).strip().splitlines()[-1] if (p.stdout + p.stderr).strip() else "")
    if p.returncode:
        sys.exit(f"failed: {' '.join(cmd)}")


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    bid, sweep, out = argv[:3]
    board = os.path.join(sweep, "best", bid + ".kicad_pcb")
    if not os.path.exists(board):
        sys.exit(f"no {board}")
    py = sys.executable
    sets = []
    i = 3
    while i < len(argv):
        if argv[i] in ("--set", "--fbfix"):
            sets += [argv[i], argv[i + 1]]
            i += 2
        else:
            i += 1
    os.makedirs(out, exist_ok=True)
    run([py, os.path.join(HERE, "fab_package.py"), board, out, "--name", bid])
    pos = os.path.join(out, bid + "-pos.csv")
    run([CLI, "pcb", "export", "pos", board, "--format", "csv", "--units", "mm", "--side", "both", "--smd-only", "-o", pos])
    run([py, os.path.join(HERE, "jlc_cpl.py"), pos, os.path.join(out, bid + "-cpl.csv")])
    run([py, os.path.join(HERE, "order_bom.py"), bid, out, *sets])


if __name__ == "__main__":
    main(sys.argv[1:])
