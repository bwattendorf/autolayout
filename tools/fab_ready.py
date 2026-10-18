"""Which finished boards could be sent to a fab? Absolute KiCad DRC counts from sweep results.

    python tools/fab_ready.py RESULTS.jsonl [RESULTS2.jsonl ...]

Reads the results.jsonl written by tools/place_sweep.py / tools/overnight.py and prints, for the best
attempt of every board: unconnected after routing, and the absolute count of each DRC kind that blocks
manufacturing (clearance, shorting, hole-to-hole, hole/copper-edge clearance, track width, dangling
items, ...), plus the other kinds KiCad reported. "Fab-ready" here means 0 unconnected and 0 of the
manufacturing kinds; kinds such as courtyard overlaps and silkscreen are listed, not counted.
"""

import json
import sys

MANUFACTURING = ["kicad_clearance", "kicad_shorting_items", "kicad_hole_to_hole", "kicad_track_dangling",
                 "kicad_via_dangling", "kicad_track_width", "kicad_copper_edge_clearance", "kicad_hole_clearance"]


def key(r):
    if r.get("status") != "ok":
        return (10**9, 10**9)
    return (int(r.get("kicad_unconnected_after") or 0), sum(int(r.get(k) or 0) for k in MANUFACTURING))


def main(argv):
    best = {}
    for path in argv:
        for line in open(path, encoding="utf-8"):
            line = line.strip()
            if not line:
                continue
            r = json.loads(line)
            b = r["board"]
            if b not in best or key(r) < key(best[b]):
                best[b] = r
    print(f"{'board':52} {'unconn':>6}  manufacturing kinds (absolute)  | other kinds")
    ready = 0
    for b in sorted(best):
        r = best[b]
        if r.get("status") != "ok":
            print(f"{b:52} {r.get('status')}")
            continue
        found = {k.replace('kicad_', ''): int(r.get(k) or 0) for k in MANUFACTURING if int(r.get(k) or 0)}
        unconnected = int(r.get("kicad_unconnected_after") or 0)
        ok = unconnected == 0 and not found
        ready += ok
        shown = str(found) if found else "none"
        print(f"{b:52} {unconnected:>6}  {shown:40} | {r.get('kicad_other_kinds') or ''}  {'FAB-READY' if ok else ''}")
    print(f"{ready} of {len(best)} boards: 0 unconnected and none of the manufacturing kinds")


if __name__ == "__main__":
    main(sys.argv[1:])
