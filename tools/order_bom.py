"""Write a JLCPCB order BOM for one board from the sourcing results.

    python tools/order_bom.py BOARD_ID OUT_DIR [--set MPN=LCSC ...]

Reads the project's `_bom/<id>_jlc_bom.csv`, maps each line to the part chosen in out/sourcing/part_*.json
(through out/bom_unique.json) and writes OUT_DIR/<id>-jlc-bom.csv (Comment, Designator, Footprint, Quantity,
LCSC) for JLCPCB assembly, plus OUT_DIR/<id>-handsource.csv for lines with no JLCPCB part. `--set MPN=LCSC`
overrides the chosen part for lines whose MPN (or comment) matches, e.g. a swap for an out-of-stock part;
overrides are printed. Prices are not written here (see tools/bom_cost.py); nothing is invented: a line
with no LCSC code goes to the hand-source file.
"""

import csv
import glob
import json
import os
import sys

BOM = "C:/Portcullis/controller/boards/_bom"
AP63203 = "C780769"  # Diodes AP63203WU-7, fixed 3.3 V; used for the regulators tools/fb_fix.py converts


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    board, out = argv[0], argv[1]
    over = {}
    if "--no-swaps" not in argv:
        swaps = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "bom_swaps.json"), encoding="utf-8"))
        over = {k: v["lcsc"] for k, v in swaps.items() if not k.startswith("_")}
    i = 2
    while i < len(argv):
        if argv[i] == "--set":
            k, v = argv[i + 1].split("=", 1)
            over[k] = v
            i += 2
        elif argv[i] == "--fbfix":
            i += 2
        else:
            i += 1
    fb = {"convert": [], "remove": []}
    if "--fbfix" in argv:
        fb = json.load(open(argv[argv.index("--fbfix") + 1], encoding="utf-8"))
    uniq = json.load(open("out/bom_unique.json", encoding="utf-8"))
    by_key = {u["key"]: i for i, u in enumerate(uniq)}
    src = {}
    for p in glob.glob("out/sourcing/part_*.json"):
        for e in json.load(open(p, encoding="utf-8")):
            src[e["index"]] = e
    os.makedirs(out, exist_ok=True)
    ok, hand = [], []
    with open(f"{BOM}/{board}_jlc_bom.csv", encoding="utf-8-sig", newline="") as f:
        for r in csv.DictReader(f):
            comment, fp = r["Comment"].strip(), r["Footprint"].strip()
            mpn, spec = r.get("MPN (for search)", "").strip(), r.get("Spec (basic-library pick)", "").strip()
            idx = by_key.get(f"{comment}|{fp}|{mpn}|{spec}")
            e = src.get(idx) if idx is not None else None
            lcsc = (e or {}).get("lcsc")
            for k, v in over.items():
                if k in (mpn, comment):
                    print(f"override {comment} ({mpn}): {lcsc} -> {v}")
                    lcsc = v
            des = [d.strip() for d in r["Designator"].split(",") if d.strip()]
            des = [d for d in des if d not in fb["remove"]]
            if not des:
                continue
            if any(d in fb["convert"] for d in des):
                conv = [d for d in des if d in fb["convert"]]
                des = [d for d in des if d not in fb["convert"]]
                ok.append(["AP63203", ",".join(conv), fp, len(conv), AP63203, "AP63203WU-7", "fixed 3.3 V (FB tied to output)"])
                if not des:
                    continue
            row = [comment, ",".join(des), fp, len(des), lcsc or ""]
            (ok if lcsc else hand).append(row + [mpn, spec])
    head = ["Comment", "Designator", "Footprint", "Quantity", "LCSC Part #"]
    for name, rows, h in (("jlc-bom", ok, head), ("handsource", hand, head + ["MPN", "Spec"])):
        with open(f"{out}/{board}-{name}.csv", "w", encoding="utf-8", newline="") as f:
            w = csv.writer(f)
            w.writerow(h)
            for row in rows:
                w.writerow(row[: len(h)])
    print(f"{board}: {len(ok)} lines with an LCSC part, {len(hand)} to hand-source -> {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
