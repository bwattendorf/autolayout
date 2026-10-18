"""Print the unique BOM lines from tools/bom_unique.py's JSON, one per line.

    python tools/bom_show.py UNIQUE.json [--mpn | --spec] [--from N --to M]
"""

import json
import sys


def main(argv):
    if not argv:
        sys.exit(__doc__)
    items = json.load(open(argv[0], encoding="utf-8"))
    sel = [(i, u) for i, u in enumerate(items)]
    if "--mpn" in argv:
        sel = [(i, u) for i, u in sel if u["mpn"]]
    if "--spec" in argv:
        sel = [(i, u) for i, u in sel if not u["mpn"]]
    lo = int(argv[argv.index("--from") + 1]) if "--from" in argv else 0
    hi = int(argv[argv.index("--to") + 1]) if "--to" in argv else len(items)
    for i, u in sel:
        if lo <= i < hi:
            print(f"{i:3} x{u['designator_count']:<3} {u['comment'][:22]:22} {u['footprint'].split(':')[-1][:28]:28} {u['mpn'][:30]:30} {u['spec'][:70]}")


if __name__ == "__main__":
    main(sys.argv[1:])
