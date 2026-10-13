"""Summarise the zones of a .kicad_pcb as KiCad wrote them, so the verifier's
zone-fill reader can be designed against real data (M7 round 3).

Plain Python 3, no pcbnew. Read-only. Usage:

    python tools/zone_probe.py BOARD.kicad_pcb [--show N]

For every `(zone ...)` prints: net, layer(s), name, whether it is a keepout,
how many `(filled_polygon ...)` it holds, how many of those carry `(island)`,
the point count and bounding box of each fill polygon (first N, default 6),
and the token names found directly inside a fill polygon. With `--show N` the
first N lines of the first filled_polygon of the first filled zone are echoed
raw. Written 2026-10-13 in a cloud session; run on a real board for the first
time on the Windows machine.
"""

import re
import sys


def blocks(text, head):
    """Yield the text of each top-level `(head ...)` list at depth 1."""
    depth, i, n = 0, 0, len(text)
    start = None
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
            if depth == 2 and text.startswith("(" + head, i) and not text[i + 1 + len(head)].isalnum() \
                    and text[i + 1 + len(head)] != "_":
                start = i
        elif c == ")":
            if depth == 2 and start is not None:
                yield text[start:i + 1]
                start = None
            depth -= 1
        i += 1


def inner(text, head):
    """Yield each `(head ...)` list found anywhere inside `text` (not nested in another `head`)."""
    out, i, n = [], 0, len(text)
    while i < n:
        j = text.find("(" + head, i)
        if j < 0:
            break
        k, depth = j, 0
        while k < n:
            if text[k] == '"':
                k += 1
                while k < n and text[k] != '"':
                    k += 2 if text[k] == "\\" else 1
            elif text[k] == "(":
                depth += 1
            elif text[k] == ")":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        out.append(text[j:k + 1])
        i = k + 1
    return out


def main(path, show):
    text = open(path, encoding="utf-8", errors="replace").read()
    zones = list(blocks(text, "zone"))
    print(f"{path}: {len(zones)} zone(s)")
    shown = False
    for z in zones:
        net = re.search(r'\(net (\d+)\)', z)
        name = re.search(r'\(net_name "([^"]*)"\)', z)
        zname = re.search(r'\(name "([^"]*)"\)', z)
        layers = re.search(r'\(layers? ([^)]*)\)', z)
        keepout = "(keepout" in z
        fills = inner(z, "filled_polygon")
        islands = sum(1 for f in fills if "(island" in f)
        print(f"- net {net.group(1) if net else '?'} {name.group(1) if name else ''!r} zone-name {zname.group(1) if zname else ''!r} "
              f"layers {layers.group(1) if layers else '?'} keepout={keepout} "
              f"filled_polygons={len(fills)} with_island_token={islands}")
        tokens = {}
        for f in fills:
            for t in re.findall(r"\((\w+)", f):
                tokens[t] = tokens.get(t, 0) + 1
        if tokens:
            print(f"    tokens inside fills: {tokens}")
        for f in fills[:6]:
            pts = [(float(a), float(b)) for a, b in re.findall(r"\(xy ([-\d.]+) ([-\d.]+)\)", f)]
            if pts:
                xs, ys = [p[0] for p in pts], [p[1] for p in pts]
                lay = re.search(r'\(layer "([^"]*)"\)', f)
                print(f"    fill layer {lay.group(1) if lay else '?'} island={'(island' in f} pts={len(pts)} "
                      f"bbox=({min(xs):.3f},{min(ys):.3f})-({max(xs):.3f},{max(ys):.3f})")
        if fills and show and not shown:
            shown = True
            print("    --- first filled_polygon, first lines, raw ---")
            for line in fills[0].splitlines()[:show]:
                print("    " + line)


if __name__ == "__main__":
    args = sys.argv[1:]
    show = 0
    if "--show" in args:
        i = args.index("--show")
        show = int(args[i + 1])
        del args[i:i + 2]
    if len(args) != 1:
        sys.exit(__doc__)
    main(args[0], show)
