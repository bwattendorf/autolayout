"""Compare two `kicad-cli pcb drc` reports of the same board: before routing
(`*_prepared_drc.rpt`) and after (`*_routed_drc.rpt`). Plain Python 3, read-only.

    python tools/drc_compare.py BEFORE.rpt AFTER.rpt [--kind clearance] [--show 8]

Prints, per violation kind, the count before, after, and how many entries in
AFTER are new (not present in BEFORE, matched on kind + the items listed with
their positions). With --kind it also lists the new entries of that kind with
their items, so one can see whether a violation involves copper the router
added (a Track or Via) or only parts that were already on the board (Pad vs
Pad). Written 2026-10-13 to explain why `autolayout verify` counted fewer
clearance violations than KiCad on 12 of 37 boards (round 8).
"""

import re
import sys


def entries(path):
    text = open(path, encoding="utf-8", errors="replace").read()
    out, cur = [], None
    for line in text.splitlines():
        m = re.match(r"^\[([a-z_0-9]+)\]: (.*)$", line)
        if m:
            cur = {"kind": m.group(1), "msg": m.group(2), "items": [], "sev": ""}
            out.append(cur)
        elif cur is not None and line.startswith("    @("):
            cur["items"].append(line.strip())
        elif cur is not None and line.startswith("    ") and not cur["sev"]:
            cur["sev"] = line.strip()
        elif line.startswith("**"):
            cur = None
    return out


def key(e):
    return (e["kind"], tuple(sorted(e["items"])))


def main(argv):
    show, kind = 8, None
    args = []
    i = 0
    while i < len(argv):
        if argv[i] == "--kind":
            kind = argv[i + 1]
            i += 2
        elif argv[i] == "--show":
            show = int(argv[i + 1])
            i += 2
        else:
            args.append(argv[i])
            i += 1
    if len(args) != 2:
        sys.exit(__doc__)
    before, after = entries(args[0]), entries(args[1])
    seen = {}
    for e in before:
        seen[key(e)] = seen.get(key(e), 0) + 1
    kinds = sorted({e["kind"] for e in before + after})
    print(f"{'kind':32} {'before':>7} {'after':>7} {'new':>6}")
    new_entries = []
    for k in kinds:
        b = sum(1 for e in before if e["kind"] == k)
        a = [e for e in after if e["kind"] == k]
        pool = dict(seen)
        new = []
        for e in a:
            if pool.get(key(e), 0) > 0:
                pool[key(e)] -= 1
            else:
                new.append(e)
        print(f"{k:32} {b:7} {len(a):7} {len(new):6}")
        if k == kind:
            new_entries = new
    for e in new_entries[:show]:
        print(f"\n[{e['kind']}] {e['msg']}  ({e['sev']})")
        for it in e["items"]:
            print("   " + it)


if __name__ == "__main__":
    main(sys.argv[1:])
