"""Convert a kicad-cli `pcb export pos --format csv` file to JLCPCB's CPL columns.

    python tools/jlc_cpl.py POS.csv OUT_CPL.csv

JLCPCB wants: Designator, Mid X, Mid Y, Layer, Rotation (Top/Bottom). Rotation is copied unchanged: JLCPCB's
parts library defines each part's zero rotation differently from KiCad's for many packages (ICs, connectors,
polarised parts), so the preview on the JLCPCB order page must be checked and rotations fixed there. This
script does not know the per-part offsets and does not guess them.
"""

import csv
import sys


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    with open(argv[0], encoding="utf-8", newline="") as f, open(argv[1], "w", encoding="utf-8", newline="") as o:
        w = csv.writer(o)
        w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
        n = 0
        for r in csv.DictReader(f):
            w.writerow([r["Ref"], r["PosX"], r["PosY"], r["Side"].capitalize(), r["Rot"]])
            n += 1
    print(f"{n} placements -> {argv[1]}")


if __name__ == "__main__":
    main(sys.argv[1:])
