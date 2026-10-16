"""Prototype (not part of the router): can a 0.4 mm-pitch QFN edge be fanned out
with staggered vias under the board's rules? Continuous geometry, no grid.

Left edge of U1 on m1/candidate4: 15 pads 0.8 x 0.2 mm at x = 95.25 (outer end
x = 94.85), y = 95.5 .. 101.1 step 0.4. Outward is -x. Each pad gets a stub:
straight out to x_b, then a 45 degree diagonal up or down, ending at a via.
Depth-first search over pads in order, smallest stub first; a stub/via is legal
if it keeps `clearance` from every pad of the part, every placed stub and via.
"""
import math, sys

TRACK, VIA, CLR = 0.1524, 0.6096, 0.1524 + 0.02  # clearance plus the router's 0.02 margin
PADS = [(95.25, 95.5 + 0.4 * k) for k in range(15)]  # centres; size 0.8 x 0.2
PW, PH = 0.8, 0.2


def seg_seg(a, b, c, d):
    def pt_seg(p, a, b):
        ax, ay = a; bx, by = b; px, py = p
        dx, dy = bx - ax, by - ay
        L = dx * dx + dy * dy
        t = 0 if L == 0 else max(0, min(1, ((px - ax) * dx + (py - ay) * dy) / L))
        return math.hypot(px - (ax + t * dx), py - (ay + t * dy))
    def inter(a, b, c, d):
        def o(p, q, r): return (q[0]-p[0])*(r[1]-p[1]) - (q[1]-p[1])*(r[0]-p[0])
        return o(a,b,c)*o(a,b,d) < 0 and o(c,d,a)*o(c,d,b) < 0
    if inter(a, b, c, d): return 0.0
    return min(pt_seg(a, c, d), pt_seg(b, c, d), pt_seg(c, a, b), pt_seg(d, a, b))


def seg_rect(a, b, cx, cy, w, h):
    x0, x1, y0, y1 = cx - w/2, cx + w/2, cy - h/2, cy + h/2
    corners = [(x0,y0),(x1,y0),(x1,y1),(x0,y1)]
    edges = list(zip(corners, corners[1:] + corners[:1]))
    inside = lambda p: x0 <= p[0] <= x1 and y0 <= p[1] <= y1
    if inside(a) or inside(b): return 0.0
    return min(seg_seg(a, b, e0, e1) for e0, e1 in edges)


def options(k):
    x, y = PADS[k]
    xe = x - PW / 2  # outer end of the pad
    for xb_off in [0.15, 0.25, 0.35, 0.5, 0.65, 0.8, 1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 3.5]:
        xb = xe - xb_off
        for sgn in (1, -1, 0):
            for dd in ([0.0] if sgn == 0 else [0.2, 0.3, 0.4, 0.5, 0.6, 0.8, 1.0, 1.2]):
                via = (xb - dd, y + sgn * dd)
                polyline = [(xe + 0.05, y), (xb, y)] + ([via] if dd else [])
                yield polyline, via, xb_off + 2 * dd


def legal(polyline, via, placed, k):
    segs = list(zip(polyline, polyline[1:]))
    # stub vs every pad except its own
    for j, (px, py) in enumerate(PADS):
        if j == k: continue
        for a, b in segs:
            if seg_rect(a, b, px, py, PW, PH) < CLR + TRACK / 2: return False
        if seg_rect(via, via, px, py, PW, PH) < CLR + VIA / 2: return False
    for (pl2, via2) in placed:
        segs2 = list(zip(pl2, pl2[1:]))
        for a, b in segs:
            for c, d in segs2:
                if seg_seg(a, b, c, d) < CLR + TRACK: return False
            if seg_seg(via2, via2, a, b) < CLR + VIA / 2 + TRACK / 2: return False
        for c, d in segs2:
            if seg_seg(via, via, c, d) < CLR + VIA / 2 + TRACK / 2: return False
        if math.dist(via, via2) < CLR + VIA: return False
    # via vs own stub is fine (its end). Own via vs its own pad: fine.
    return True


def static_ok(polyline, via, k):
    segs = list(zip(polyline, polyline[1:]))
    for j, (px, py) in enumerate(PADS):
        if j == k: continue
        for a, b in segs:
            if seg_rect(a, b, px, py, PW, PH) < CLR + TRACK / 2: return False
        if seg_rect(via, via, px, py, PW, PH) < CLR + VIA / 2: return False
    return True


def pair_ok(o1, o2):
    (pl1, v1), (pl2, v2) = o1, o2
    s1, s2 = list(zip(pl1, pl1[1:])), list(zip(pl2, pl2[1:]))
    for a, b in s1:
        for c, d in s2:
            if seg_seg(a, b, c, d) < CLR + TRACK: return False
        if seg_seg(v2, v2, a, b) < CLR + VIA / 2 + TRACK / 2: return False
    for c, d in s2:
        if seg_seg(v1, v1, c, d) < CLR + VIA / 2 + TRACK / 2: return False
    return math.dist(v1, v2) >= CLR + VIA


def solve(order, limit_s=120):
    import time
    t0 = time.time()
    OPTS = {}
    for k in range(len(PADS)):
        o = [(pl, v, c) for pl, v, c in options(k) if static_ok(pl, v, k)]
        o.sort(key=lambda x: x[2])
        OPTS[k] = o[:400]
    sol, placed, nodes = {}, [], [0]
    cache = {}
    def comp(k1, i1, k2, i2):
        key = (k1, i1, k2, i2)
        if key not in cache:
            cache[key] = pair_ok((OPTS[k1][i1][0], OPTS[k1][i1][1]), (OPTS[k2][i2][0], OPTS[k2][i2][1]))
        return cache[key]
    def rec(i):
        nodes[0] += 1
        if time.time() - t0 > limit_s: return False
        if i == len(order): return True
        k = order[i]
        for oi in range(len(OPTS[k])):
            if all(comp(k2, i2, k, oi) for k2, i2 in placed):
                placed.append((k, oi)); sol[k] = OPTS[k][oi][:2]
                if rec(i + 1): return True
                placed.pop(); del sol[k]
        return False
    ok = rec(0)
    return ok, sol, nodes[0], {k: len(v) for k, v in OPTS.items()}


if __name__ == "__main__":
    for name, order in (("outside-in", [0,14,1,13,2,12,3,11,4,10,5,9,6,8,7]),
                        ("middle-out", [7,6,8,5,9,4,10,3,11,2,12,1,13,0,14]),
                        ("left-to-right", list(range(15)))):
        ok, sol, n, nopt = solve(order)
        print(name, "feasible" if ok else "NOT found", "nodes", n, "options per pad", sorted(set(nopt.values())))
        if ok:
            for k in sorted(sol): print(" pad", k + 1, "via at (%.3f, %.3f)" % sol[k][1])
            break
