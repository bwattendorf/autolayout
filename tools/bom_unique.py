"""Collapse the per-board JLCPCB BOMs into the unique parts that need sourcing.

    python tools/bom_unique.py OUT.json [BOM_DIR]

A part is identified by (value/comment, footprint, MPN, spec text). Writes OUT.json: a list of
{key, comment, footprint, mpn, spec, boards, designator_count} sorted by how many parts use it, and prints
the counts. Lines with an LCSC part number already are kept with it.
"""

import csv
import glob
import json
import os
import sys

DEFAULT = "C:/Portcullis/controller/boards/_bom"


def main(argv):
    if not argv:
        sys.exit(__doc__)
    out = argv[0]
    bom_dir = argv[1] if len(argv) > 1 else DEFAULT
    uniq = {}
    for p in sorted(glob.glob(os.path.join(bom_dir, "*_jlc_bom.csv"))):
        board = os.path.basename(p).split("_jlc_bom")[0]
        with open(p, encoding="utf-8-sig", newline="") as f:
            for r in csv.DictReader(f):
                comment, fp = r["Comment"].strip(), r["Footprint"].strip()
                mpn, spec = r.get("MPN (for search)", "").strip(), r.get("Spec (basic-library pick)", "").strip()
                lcsc = r.get("LCSC Part #", "").strip()
                key = f"{comment}|{fp}|{mpn}|{spec}"
                n = len([d for d in r["Designator"].split(",") if d.strip()])
                u = uniq.setdefault(key, {"key": key, "comment": comment, "footprint": fp, "mpn": mpn, "spec": spec,
                                          "lcsc": lcsc, "boards": {}, "designator_count": 0})
                u["boards"][board] = u["boards"].get(board, 0) + n
                u["designator_count"] += n
    items = sorted(uniq.values(), key=lambda u: -u["designator_count"])
    json.dump(items, open(out, "w", encoding="utf-8"), indent=1)
    with_mpn = [u for u in items if u["mpn"]]
    print(f"{len(items)} unique lines ({len(with_mpn)} with an MPN, {len(items) - len(with_mpn)} spec only); "
          f"{sum(u['designator_count'] for u in items)} parts in total")
    unique_mpn = {u["mpn"] for u in with_mpn}
    print(f"{len(unique_mpn)} distinct MPNs")


if __name__ == "__main__":
    main(sys.argv[1:])
