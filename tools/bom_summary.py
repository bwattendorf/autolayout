"""Summarise the project's JLCPCB BOM files: how many lines name an LCSC part, an MPN, or only a spec.

    python tools/bom_summary.py [BOM_DIR] [--list-missing BOARD_ID]

BOM_DIR defaults to the Portcullis `_bom` folder. For every `<id>_jlc_bom.csv` prints the number of BOM
lines, the part count (designators), the lines with an LCSC part number, with only an MPN, and with
neither (a spec line: the part still has to be picked). `--list-missing ID` prints those lines for one board.
"""

import csv
import glob
import os
import sys

DEFAULT = "C:/Portcullis/controller/boards/_bom"


def rows(path):
    with open(path, encoding="utf-8-sig", newline="") as f:
        return list(csv.DictReader(f))


def main(argv):
    bom_dir = next((a for a in argv if not a.startswith("--") and os.path.isdir(a)), DEFAULT)
    tot = {"lines": 0, "parts": 0, "lcsc": 0, "mpn": 0, "spec": 0}
    unique_lcsc = set()
    for p in sorted(glob.glob(os.path.join(bom_dir, "*_jlc_bom.csv"))):
        board = os.path.basename(p).split("_jlc_bom")[0]
        rs = rows(p)
        n_parts = sum(len([d for d in r["Designator"].split(",") if d.strip()]) for r in rs)
        lcsc = [r for r in rs if r.get("LCSC Part #", "").strip()]
        mpn = [r for r in rs if not r.get("LCSC Part #", "").strip() and r.get("MPN (for search)", "").strip()]
        spec = [r for r in rs if not r.get("LCSC Part #", "").strip() and not r.get("MPN (for search)", "").strip()]
        for r in lcsc:
            unique_lcsc.add(r["LCSC Part #"].strip())
        print(f"{board:6} {len(rs):4} lines {n_parts:4} parts | LCSC given {len(lcsc):3} | MPN only {len(mpn):3} | spec only {len(spec):3}")
        for k, v in (("lines", len(rs)), ("parts", n_parts), ("lcsc", len(lcsc)), ("mpn", len(mpn)), ("spec", len(spec))):
            tot[k] += v
        if "--list-missing" in argv and argv[argv.index("--list-missing") + 1] == board:
            for r in mpn + spec:
                print("   ", r["Comment"], "|", r["Designator"][:40], "|", r["Footprint"], "|", r.get("MPN (for search)", ""), "|", r.get("Spec (basic-library pick)", "")[:90])
    print(f"total: {tot['lines']} lines, {tot['parts']} parts; LCSC given on {tot['lcsc']} lines ({len(unique_lcsc)} unique), MPN only {tot['mpn']}, spec only {tot['spec']}")


if __name__ == "__main__":
    main(sys.argv[1:])
