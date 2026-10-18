"""Replace LCSC part numbers in existing order BOMs (keeps earlier --set overrides intact).

    python tools/bom_patch.py OLD=NEW [OLD=NEW ...]

Edits every out/order/*/*-jlc-bom.csv in place (these are generated files, not inputs) and prints the
number of lines changed per file.
"""

import csv
import glob
import sys


def main(argv):
    if not argv:
        sys.exit(__doc__)
    m = dict(a.split("=", 1) for a in argv)
    total = 0
    for p in sorted(glob.glob("out/order/*/*-jlc-bom.csv")):
        rows = list(csv.reader(open(p, encoding="utf-8", newline="")))
        n = 0
        for r in rows[1:]:
            if len(r) > 4 and r[4] in m:
                r[4] = m[r[4]]
                n += 1
        if n:
            with open(p, "w", encoding="utf-8", newline="") as f:
                csv.writer(f).writerows(rows)
            print(f"{p}: {n}")
            total += n
    print(f"lines changed: {total}")


if __name__ == "__main__":
    main(sys.argv[1:])
