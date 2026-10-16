"""Prototype (not part of the router): can one 0.4 mm-pitch QFN edge be tapered onto
the 0.3098 mm routing grid without vias? Continuous geometry, no router.

Left edge of U1 on m1/candidate4: 15 pads 0.8 x 0.2 mm at x = 95.25, y = 95.5 .. 101.1
step 0.4, outward is -x. Pad k leaves straight out to x_b, runs a 45 degree diagonal
to a grid row y_g, then straight on to a common landing column x_L (the normal grid
router continues from there). Rows are y_g = g0 + j * 0.3098; the board's real phase
g0 is not known here, so it is swept. Rows are assigned in pad order (no crossing).
Depth-first search; legal = clearance 0.1524 + 0.02 margin to every other pad and
every other stub.
"""
import math, sys, time
sys.path.insert(0, __file__.rsplit("/", 1)[0] if "/" in __file__ else ".")
from fanout_proto import seg_seg, seg_rect, PADS, PW, PH, TRACK

CLR = 0.1524 + 0.005  # the grid's own margin (pitch = track + clearance + max_error 0.005), so adjacent rows are legal

PITCH = 0.3098
XL_OFF = 7.0  # landing column this far beyond the pad ends


def options(k, g0):
    x, y = PADS[k]
    xe = x - PW / 2
    j0 = math.floor((y - g0) / PITCH)
    for j in range(j0 - 4, j0 + 6):
        yg = g0 + j * PITCH
        dy = yg - y
        for xb_off in [0.1, 0.2, 0.3, 0.4, 0.5, 0.65, 0.8, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0]:
            xb = xe - xb_off
            pl = [(xe + 0.05, y), (xb, y)]
            if abs(dy) > 1e-9:
                pl.append((xb - abs(dy), yg))
            pl.append((xe - XL_OFF, yg))
            yield j, pl, xb_off + abs(dy)


def static_ok(pl, k):
    for a, b in zip(pl, pl[1:]):
        for j, (px, py) in enumerate(PADS):
            if j != k and seg_rect(a, b, px, py, PW, PH) < CLR + TRACK / 2:
                return False
    return True


def pair_ok(p1, p2):
    return all(seg_seg(a, b, c, d) >= CLR + TRACK
               for a, b in zip(p1, p1[1:]) for c, d in zip(p2, p2[1:]))


def solve(g0, limit_s=20):
    opts = []
    for k in range(len(PADS)):
        o = [(j, pl, c) for j, pl, c in options(k, g0) if static_ok(pl, k)]
        o.sort(key=lambda t: t[2])
        opts.append(o)
    t0 = time.time()  # the cap covers the search only, not building the options
    best = [0, None]
    sol = []
    def rec(k):
        if time.time() - t0 > limit_s: return False
        if k > best[0]: best[0], best[1] = k, list(sol)
        if k == len(PADS): return True
        for j, pl, _ in opts[k]:
            if sol and j <= sol[-1][0]: continue  # strictly increasing rows
            if all(pair_ok(pl, p2) for _, p2 in sol):
                sol.append((j, pl))
                if rec(k + 1): return True
                sol.pop()
        return False
    ok = rec(0)
    return ok, best[0], [len(o) for o in opts]


if __name__ == "__main__":
    for step in range(0, 10, 2):
        g0 = step * PITCH / 10
        ok, depth, n = solve(g0)
        print(f"grid phase {g0:.4f}: {'ALL 15 tapered' if ok else f'stuck after {depth} pads'}  (options per pad {min(n)}-{max(n)})", flush=True)
