"""Rank BOM lines by cost, to pick which parts are worth looking for cheaper substitutes.

    python tools/bom_rank.py [--top N] [--out out/bom_rank.json]

Reads out/bom_unique.json and out/sourcing/part_*.json. Cost per line = single-unit price (pcbparts lookup,
measured) x placements summed over all boards (one of each board). Extended lines also carry the $3 per-order
fee (estimate, pcbparts docs). Prints rank, cost, library type, stock, model, manufacturer, boards.
"""

import glob
import json
import sys


def main(argv):
    top = int(argv[argv.index("--top") + 1]) if "--top" in argv else 40
    out = argv[argv.index("--out") + 1] if "--out" in argv else "out/bom_rank.json"
    uniq = json.load(open("out/bom_unique.json", encoding="utf-8"))
    src = {}
    for p in sorted(glob.glob("out/sourcing/part_*.json")):
        for e in json.load(open(p, encoding="utf-8")):
            src[e["index"]] = e
    rows = []
    for i, u in enumerate(uniq):
        e = src.get(i)
        if not e or e.get("unit_price_usd") is None:
            continue
        n = sum(u["boards"].values())
        rows.append({"index": i, "comment": u["comment"], "footprint": u["footprint"], "qty_all": n,
                     "boards": len(u["boards"]), "lcsc": e["lcsc"], "model": e["model"],
                     "mfr": e.get("manufacturer"), "type": e.get("library_type"), "stock": e.get("stock"),
                     "unit": e["unit_price_usd"], "total": round(n * float(e["unit_price_usd"]), 2)})
    rows.sort(key=lambda r: -r["total"])
    for k, r in enumerate(rows[:top]):
        print(f"{k+1:3} ${r['total']:7.2f} {r['type'][:3]:3} x{r['qty_all']:<4} {r['boards']:2}b ${r['unit']:<7} "
              f"stock {r['stock']:<9} {r['lcsc']:<10} {r['model']} | {r['mfr']} | {r['comment']}")
    ext = [r for r in rows if r["type"] == "extended"]
    print(f"lines {len(rows)}; extended {len(ext)}; top {top} = ${sum(r['total'] for r in rows[:top]):.2f} "
          f"of ${sum(r['total'] for r in rows):.2f}")
    json.dump(rows, open(out, "w", encoding="utf-8"), indent=1)


if __name__ == "__main__":
    main(sys.argv[1:])
