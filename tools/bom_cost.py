"""Merge the sourcing results with the unique BOM and estimate parts cost per board.

    python tools/bom_cost.py [--qty N] [--out out/bom_cost.json]

Reads out/bom_unique.json and out/sourcing/part_*.json (each entry has index, lcsc, library_type, stock,
unit_price_usd, status). Prices are the single-unit prices the lookup tool returned (measured from the tool
output, not quotes for volume). The extended-part fee ($3 per distinct extended part per order) is from the
pcbparts tool documentation (an estimate, not a JLCPCB quote). Lines with no part chosen are listed and not
costed. Boards are built QTY at a time (default 1 of each board).
"""

import glob
import json
import sys

EXT_FEE = 3.0


def main(argv):
    qty = int(argv[argv.index("--qty") + 1]) if "--qty" in argv else 1
    out = argv[argv.index("--out") + 1] if "--out" in argv else "out/bom_cost.json"
    uniq = json.load(open("out/bom_unique.json", encoding="utf-8"))
    src = {}
    for p in sorted(glob.glob("out/sourcing/part_*.json")):
        for e in json.load(open(p, encoding="utf-8")):
            src[e["index"]] = e
    boards = {}
    missing = []
    ext_parts = set()
    total_lines = 0
    for i, u in enumerate(uniq):
        e = src.get(i)
        price = e.get("unit_price_usd") if e else None
        if not e or not e.get("lcsc") or price is None:
            missing.append((i, u["comment"], u["mpn"], u["designator_count"], e["status"] if e else "unsourced"))
            continue
        total_lines += 1
        if e.get("library_type") == "extended":
            ext_parts.add(e["lcsc"])
        for b, n in u["boards"].items():
            d = boards.setdefault(b, {"parts_usd": 0.0, "ext": set(), "placements": 0})
            d["parts_usd"] += float(price) * n
            d["placements"] += n
            if e.get("library_type") == "extended":
                d["ext"].add(e["lcsc"])
    rows = []
    for b, d in sorted(boards.items()):
        rows.append({"board": b, "parts_usd": round(d["parts_usd"], 2), "placements": d["placements"],
                     "extended_distinct": len(d["ext"]), "extended_fee_usd": EXT_FEE * len(d["ext"])})
    for r in rows:
        print(f"{r['board']:6} parts ${r['parts_usd']:8.2f}  placements {r['placements']:4}  extended {r['extended_distinct']:3} (fee ${r['extended_fee_usd']:.0f})")
    tot = sum(r["parts_usd"] for r in rows) * qty
    print(f"one of each board x{qty}: parts ${tot:.2f}; distinct extended parts over all boards {len(ext_parts)} "
          f"(fee ${EXT_FEE * len(ext_parts):.0f} if ordered together, estimate)")
    print(f"costed lines {total_lines}; NOT costed {len(missing)}:")
    for m in missing:
        print("  ", m, "boards:", ",".join(sorted(uniq[m[0]]["boards"])))
    blocked = {b for m in missing for b in uniq[m[0]]["boards"]}
    print("boards with every line sourced:", ",".join(sorted(b for b in boards if b not in blocked)))
    json.dump({"boards": rows, "missing": missing, "qty": qty}, open(out, "w", encoding="utf-8"), indent=1)


if __name__ == "__main__":
    main(sys.argv[1:])
