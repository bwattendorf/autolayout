"""Existence check (not part of the router): does a legal escape exist for every
pad of one part, in continuous geometry on a fine lattice?

    python tools/escape_exist.py BOARD.kicad_pcb REF [--margin 3.5] [--step 0.05]

Window = the part's pad box plus --margin. Lattice step --step (default 0.05 mm,
aligned to the part's pads). Two copper layers (F.Cu, B.Cu), 8-direction moves,
vias anywhere legal. Every pad with a net must reach the window edge; a pad on a
plane net (named with --plane NET, repeatable) must instead reach a legal via
site. Foreign pads, other escapes, and vias are obstacles with the board's
clearance (read from the .kicad_pro beside the board). Negotiated congestion
(PathFinder) over all pads of the part at once.

Answers one question: does a legal escape exist at all under these rules, for
the real pads around the part? It does not route the nets' real destinations,
the plane pours, or anything outside the window. On success it writes the
escapes (segments and vias) into a copy of the board, to be judged by KiCad DRC.
"""
import heapq, json, math, os, re, sys
import numpy as np

# ---------- s-expression reader -------------------------------------------
def tokens(text):
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c in ' \t\r\n':
            i += 1
        elif c in '()':
            yield c; i += 1
        elif c == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == chr(92) else 1
            yield ('s', text[i + 1:j]); i = j + 1
        else:
            j = i
            while j < n and text[j] not in ' \t\r\n()':
                j += 1
            yield ('a', text[i:j]); i = j

def parse(text):
    stack, cur = [], []
    for t in tokens(text):
        if t == '(':
            stack.append(cur); cur = []
        elif t == ')':
            done = cur; cur = stack.pop(); cur.append(done)
        else:
            cur.append(t[1])
    return cur[0]

def kids(l, key):
    return [x for x in l if isinstance(x, list) and x and x[0] == key]

def kid(l, key):
    k = kids(l, key)
    return k[0] if k else None

def num(x):
    return float(x)

# ---------- board -----------------------------------------------------------
class Pad:
    pass

def load_pads(path):
    root = parse(open(path, encoding='utf8').read())
    nets = {}
    for n in kids(root, 'net'):
        if len(n) >= 3 and re.fullmatch(r'-?\d+', n[1]):
            nets[int(n[1])] = n[2]
    pads, holes = [], []
    for fp in kids(root, 'footprint'):
        at = kid(fp, 'at'); fx, fy = num(at[1]), num(at[2]); frot = num(at[3]) if len(at) > 3 else 0.0
        ref = next((p[2] for p in kids(fp, 'property') if p[1] == 'Reference'), '?')
        side = kid(fp, 'layer')[1]
        a = math.radians(frot)
        for pd in kids(fp, 'pad'):
            p = Pad()
            p.ref, p.num, p.kind, p.shape = ref, pd[1], pd[2], pd[3]
            pat = kid(pd, 'at'); px, py = num(pat[1]), num(pat[2]); prot = num(pat[3]) if len(pat) > 3 else 0.0
            p.x = fx + px * math.cos(a) + py * math.sin(a)
            p.y = fy - px * math.sin(a) + py * math.cos(a)
            sz = kid(pd, 'size'); p.w, p.h = num(sz[1]), num(sz[2])
            p.rot = math.radians(prot)
            rr = kid(pd, 'roundrect_rratio'); p.rr = num(rr[1]) if rr else 0.0
            ls = kid(pd, 'layers')[1:]
            p.layers = set()
            for l in ls:
                if l == '*.Cu': p.layers |= {0, 1}
                elif l == 'F&B.Cu': p.layers |= {0, 1}
                elif l == 'F.Cu': p.layers.add(0)
                elif l == 'B.Cu': p.layers.add(1)
            nt = kid(pd, 'net')
            if nt is None: p.net = None
            elif re.fullmatch(r'-?\d+', nt[1]) and len(nt) > 2: p.net = nt[2]
            elif re.fullmatch(r'-?\d+', nt[1]): p.net = nets.get(int(nt[1]))
            else: p.net = nt[1]
            pads.append(p)
    return root, nets, pads

def pad_dist(p, X, Y):
    """Signed distance from points (X, Y) to the pad's copper (negative inside)."""
    c, s = math.cos(-p.rot), math.sin(-p.rot)
    dx, dy = X - p.x, Y - p.y
    u, v = dx * c - dy * s, dx * s + dy * c
    hw, hh = p.w / 2, p.h / 2
    if p.shape == 'circle':
        return np.hypot(u, v) - hw
    if p.shape == 'oval':
        r = min(hw, hh)
        if hw >= hh:
            ux = np.clip(u, -(hw - r), hw - r); return np.hypot(u - ux, v) - r
        vy = np.clip(v, -(hh - r), hh - r); return np.hypot(u, v - vy) - r
    r = p.rr * min(p.w, p.h) if p.shape == 'roundrect' else 0.0
    qx, qy = np.abs(u) - (hw - r), np.abs(v) - (hh - r)
    return np.hypot(np.maximum(qx, 0), np.maximum(qy, 0)) + np.minimum(np.maximum(qx, qy), 0) - r


# ---------- rules ----------------------------------------------------------
def load_rules(pcb):
    pro = json.load(open(os.path.splitext(pcb)[0] + '.kicad_pro', encoding='utf8'))
    ds = pro['board']['design_settings']['rules']
    classes = pro['net_settings']['classes']
    d = next(c for c in classes if c['name'] == 'Default')
    clr = max([c['clearance'] for c in classes] + [ds.get('min_clearance', 0)])
    return dict(track=d['track_width'], clr=clr, via=d['via_diameter'], drill=d['via_drill'],
                h2h=ds.get('min_hole_to_hole', 0.25), margin=ds.get('max_error', 0.005))


# ---------- fine lattice ---------------------------------------------------
class Lattice:
    def __init__(self, pads, ref, rules, margin, step, planes, disc=0.03, slack=0.005, grid=None, sub=6):
        self.h = step
        self.r = rules
        part = [p for p in pads if p.ref == ref]
        xs, ys = [p.x for p in part], [p.y for p in part]
        self.grid = grid
        if grid:
            # Router-frame mode: lattice step = grid pitch / sub, anchored at the router's grid origin,
            # so every sub-th lattice line is a router grid line and the crossings are its nodes.
            (gx, gy), gp = grid
            self.h = step = gp / sub
            self.sub = sub
            # Window edges lie exactly on router grid lines, so the outermost lattice line is a line of
            # router nodes with only free field beyond it: a plan that ends there leaves the router room.
            ax = math.floor((min(xs) - margin - gx) / gp) * sub
            ay = math.floor((min(ys) - margin - gy) / gp) * sub
            bx = math.ceil((max(xs) + margin - gx) / gp) * sub
            by = math.ceil((max(ys) + margin - gy) / gp) * sub
            self.x0, self.y0 = gx + ax * step, gy + ay * step
            self.ax0, self.ay0 = ax, ay
            self.W_fixed, self.H_fixed = bx - ax + 1, by - ay + 1
        else:
            self.x0 = math.floor((min(xs) - margin) / step) * step
            self.y0 = math.floor((min(ys) - margin) / step) * step
        if grid:
            self.W, self.H = self.W_fixed, self.H_fixed
        else:
            self.W = int(math.ceil((max(xs) + margin - self.x0) / self.h)) + 1
            self.H = int(math.ceil((max(ys) + margin - self.y0) / self.h)) + 1
        self.part = part
        self.planes = set(planes)
        self.netid = {}
        for p in pads:
            if p.net and p.net not in self.netid:
                self.netid[p.net] = len(self.netid) + 1
        r = rules
        m = r['margin'] + slack
        self.need_track = r['track'] / 2 + r['clr'] + m                # track centre to foreign pad
        self.need_via = r['via'] / 2 + r['clr'] + m                    # via centre to foreign pad
        self.tt = r['track'] + r['clr'] + m + disc                     # track centre to track centre
        self.vt = r['via'] / 2 + r['clr'] + r['track'] / 2 + m + disc  # via centre to track centre
        self.vv = max(r['via'] + r['clr'], r['drill'] + r['h2h']) + m + disc
        X = self.x0 + np.arange(self.W) * step
        Y = self.y0 + np.arange(self.H) * step
        self.X, self.Y = np.meshgrid(X, Y)
        self.goal = None
        if grid:
            ii = (np.arange(self.W) + self.ax0) % self.sub == 0
            jj = (np.arange(self.H) + self.ay0) % self.sub == 0
            node = np.outer(jj, ii)
            ring = np.zeros((self.H, self.W), dtype=bool)
            ring[0, :] = True; ring[-1, :] = True; ring[:, 0] = True; ring[:, -1] = True   # the outermost line only
            self.goal = node & ring
            self.goal_list = self.goal.ravel().tolist()
        self._build_obstacles(pads)

    def _top2(self, pads, X, Y, reach):
        """Per layer: nearest pad distance with its net (d1, n1) and nearest of a different net (d2)."""
        H, W = X.shape
        step_x = X[0, 1] - X[0, 0] if W > 1 else 1.0
        step_y = Y[1, 0] - Y[0, 0] if H > 1 else 1.0
        x0, y0 = X[0, 0], Y[0, 0]
        INF = 1e9
        d1 = [np.full((H, W), INF) for _ in range(2)]
        n1 = [np.zeros((H, W), dtype=np.int32) for _ in range(2)]
        d2 = [np.full((H, W), INF) for _ in range(2)]
        n2 = [np.zeros((H, W), dtype=np.int32) for _ in range(2)]
        for p in pads:
            q = self.netid.get(p.net, -1) if p.net else -1
            ext = max(p.w, p.h) / 2 * 1.5 + reach
            if p.x < x0 - ext or p.x > x0 + W * step_x + ext or p.y < y0 - ext or p.y > y0 + H * step_y + ext:
                continue
            i0 = max(0, int((p.x - ext - x0) / step_x)); i1 = min(W, int((p.x + ext - x0) / step_x) + 2)
            j0 = max(0, int((p.y - ext - y0) / step_y)); j1 = min(H, int((p.y + ext - y0) / step_y) + 2)
            if i0 >= i1 or j0 >= j1:
                continue
            dp = np.maximum(pad_dist(p, X[j0:j1, i0:i1], Y[j0:j1, i0:i1]), 0.0)
            for l in p.layers:
                a1, b1, a2, b2 = d1[l][j0:j1, i0:i1], n1[l][j0:j1, i0:i1], d2[l][j0:j1, i0:i1], n2[l][j0:j1, i0:i1]
                same1 = b1 == q
                same2 = b2 == q
                m_a = same1 & (dp < a1)
                m_b = (~same1) & same2 & (dp < a2)
                m_c = (~same1) & (~same2) & (dp < a1)
                m_d = (~same1) & (~same2) & (~(dp < a1)) & (dp < a2)
                old_d1, old_n1 = a1.copy(), b1.copy()
                a1[m_a] = dp[m_a]
                a2[m_b] = dp[m_b]
                a2[m_c] = old_d1[m_c]; b2[m_c] = old_n1[m_c]
                a1[m_c] = dp[m_c]; b1[m_c] = q
                a2[m_d] = dp[m_d]; b2[m_d] = q
                sw = m_b & (a2 < a1)
                t = a1[sw].copy(); tb = b1[sw].copy()
                a1[sw] = a2[sw]; b1[sw] = b2[sw]; a2[sw] = t; b2[sw] = tb
        return d1, n1, d2

    def _build_obstacles(self, pads):
        self.d1, self.n1, self.d2 = self._top2(pads, self.X, self.Y, self.need_via + 0.5)
        self.all_pads = pads
        if self.grid:
            # Board-wide node maps (one entry per router grid node), to tell which nodes the router can leave from.
            (gx, gy), gp = self.grid
            xs = [p.x for p in pads]; ys = [p.y for p in pads]
            nx_ = int((max(xs) + 3 - gx) / gp) + 1
            ny_ = int((max(ys) + 3 - gy) / gp) + 1
            NX, NY = np.meshgrid(gx + np.arange(nx_) * gp, gy + np.arange(ny_) * gp)
            self.gnode = (NX, NY)
            self.gd1, self.gn1, self.gd2 = self._top2(pads, NX, NY, self.need_via + 0.5)
            self._open_cache = {}

    def open_nodes(self, net, min_size=200):
        """Per layer, boolean map over all router nodes: free for `net` (clear of foreign pads) and part of a
        4-connected free region of at least `min_size` nodes, i.e. nodes the router can really leave from."""
        if net in self._open_cache:
            return self._open_cache[net]
        from scipy import ndimage
        q = self.netid.get(net, 0)
        out = []
        for l in range(2):
            f = np.where(self.gn1[l] == q, self.gd2[l], self.gd1[l])
            free = f >= self.need_track
            lab, n = ndimage.label(free)
            sizes = np.bincount(lab.ravel())
            big = sizes >= min_size
            big[0] = False
            out.append(big[lab])
        self._open_cache[net] = out
        return out

    def goal_cells(self, net):
        """Per layer, flat list: lattice cells on the window's outermost line that are router nodes the router
        can leave from. None when not in router-frame mode."""
        if self.goal is None:
            return None
        key = ('goal', net)
        if key in self._open_cache:
            return self._open_cache[key]
        op = self.open_nodes(net)
        H, W = self.H, self.W
        res = []
        for l in range(2):
            g = np.zeros((H, W), dtype=bool)
            jj, ii = np.nonzero(self.goal)
            ni = (ii + self.ax0) // self.sub
            nj = (jj + self.ay0) // self.sub
            ok = (ni >= 0) & (nj >= 0) & (ni < op[l].shape[1]) & (nj < op[l].shape[0])
            sel = np.zeros(len(ii), dtype=bool)
            sel[ok] = op[l][nj[ok], ni[ok]]
            g[jj[sel], ii[sel]] = True
            res.append(g.ravel().tolist())
        self._open_cache[key] = res
        return res

    def foreign(self, net):
        q = self.netid.get(net, 0)
        return [np.where(self.n1[l] == q, self.d2[l], self.d1[l]) for l in range(2)]

    def inner(self, p):
        """Cells whose centre lies in the pad with a track end fully on copper."""
        h = self.h
        i0 = max(0, int((p.x - p.w - self.x0) / h)); i1 = min(self.W, int((p.x + p.w - self.x0) / h) + 2)
        j0 = max(0, int((p.y - p.w - self.y0) / h)); j1 = min(self.H, int((p.y + p.w - self.y0) / h) + 2)
        sd = pad_dist(p, self.X[j0:j1, i0:i1], self.Y[j0:j1, i0:i1])
        out = []
        for inset in (self.r['track'] / 2, 0.0):
            jj, ii = np.nonzero(sd <= -inset - 1e-9) if inset > 0 else np.nonzero(sd <= 1e-9)
            out = [(j0 + a, i0 + b) for a, b in zip(jj, ii)]
            if out:
                break
        return out


# ---------- negotiated router ----------------------------------------------
class Router:
    def __init__(self, L):
        self.L = L
        H, W = L.H, L.W
        self.tz = [np.zeros((H, W), dtype=np.int16) for _ in range(2)]   # track-move zones of all nets
        self.vz = [np.zeros((H, W), dtype=np.int16) for _ in range(2)]   # via-site zones
        self.hist = [np.zeros((H, W), dtype=np.float32) for _ in range(2)]
        self.route = {}      # pad key -> dict(cells=[(l,j,i)], vias=[(j,i)])
        self.nz = {}         # net -> ([tz per layer], [vz per layer]): that net's own stamps, ignored by its own routes
        self.kern = {}
        self.hz = np.zeros((H, W), dtype=np.int16)   # hole-to-hole zones: apply to vias of any net, own net included
        vh = L.r['drill'] + L.r['h2h'] + L.r['margin'] + 0.01
        for name, rad in (('tt', L.tt), ('vt', L.vt), ('vv', L.vv), ('vh', vh)):
            R = int(math.ceil(rad / L.h))
            yy, xx = np.mgrid[-R:R + 1, -R:R + 1]
            self.kern[name] = ((np.hypot(yy, xx) * L.h) < rad).astype(np.int16), R

    def _stamp(self, arr, j, i, name, sign):
        k, R = self.kern[name]
        H, W = self.L.H, self.L.W
        j0, j1, i0, i1 = max(0, j - R), min(H, j + R + 1), max(0, i - R), min(W, i + R + 1)
        arr[j0:j1, i0:i1] += sign * k[j0 - (j - R):j1 - (j - R), i0 - (i - R):i1 - (i - R)]

    def own(self, net):
        if net not in self.nz:
            H, W = self.L.H, self.L.W
            self.nz[net] = ([np.zeros((H, W), dtype=np.int16) for _ in range(2)],
                            [np.zeros((H, W), dtype=np.int16) for _ in range(2)])
        return self.nz[net]

    def stamp_route(self, rt, sign, net):
        otz, ovz = self.own(net)
        for (l, j, i) in rt['cells']:
            for a in (self.tz[l], otz[l]):
                self._stamp(a, j, i, 'tt', sign)
            for a in (self.vz[l], ovz[l]):
                self._stamp(a, j, i, 'vt', sign)
        for (j, i) in rt['vias']:
            self._stamp(self.hz, j, i, 'vh', sign)
            for l in range(2):
                for a in (self.tz[l], otz[l]):
                    self._stamp(a, j, i, 'vt', sign)
                for a in (self.vz[l], ovz[l]):
                    self._stamp(a, j, i, 'vv', sign)

    def conflicts(self, rt, net):
        otz, ovz = self.own(net)
        c = 0
        for (l, j, i) in rt['cells']:
            c += int(self.tz[l][j, i] - otz[l][j, i] > 0)
        for (j, i) in rt['vias']:
            c += int((self.vz[0][j, i] - ovz[0][j, i] > 0) or (self.vz[1][j, i] - ovz[1][j, i] > 0))
        return c

    def search(self, pad, plane, pres, via_cost=14.0):
        L = self.L
        H, W = L.H, L.W
        f = L.foreign(pad.net)
        ok = [(f[l] >= L.need_track).ravel().tolist() for l in range(2)]
        viaok = ((f[0] >= L.need_via) & (f[1] >= L.need_via) & (self.hz == 0)).ravel().tolist()
        otz, ovz = self.own(pad.net)
        mult = [(1.0 + pres * (self.tz[l] - otz[l]) + self.hist[l]).ravel().tolist() for l in range(2)]
        vmult = (pres * ((self.vz[0] - ovz[0]) + (self.vz[1] - ovz[1]))).ravel().tolist()
        starts = [(l, j, i) for l in (pad.layers or {0}) for (j, i) in L.inner(pad) if ok[l][j * W + i]]
        if not starts:
            return None
        INF = 1e18
        dist = [dict(), dict()]
        prev = {}
        heap = []
        SQ = math.sqrt(2.0)

        band = 0
        if L.goal is not None and not any(any(g) for g in L.goal_cells(pad.net)):
            return None
        goals = L.goal_cells(pad.net)

        def hfun(j, i):
            return 0.0 if plane else float(max(0, min(i, W - 1 - i, j, H - 1 - j) - band))

        for (l, j, i) in starts:
            dist[l][j * W + i] = 0.0
            heapq.heappush(heap, (hfun(j, i), 0.0, l, j * W + i))
        goal = None
        nb = ((0, 1, 1.0), (0, -1, 1.0), (1, 0, 1.0), (-1, 0, 1.0), (1, 1, SQ), (1, -1, SQ), (-1, 1, SQ), (-1, -1, SQ))
        while heap:
            fc, g, l, idx = heapq.heappop(heap)
            if g > dist[l].get(idx, INF):
                continue
            j, i = divmod(idx, W)
            if plane:
                if viaok[idx]:
                    goal = (l, idx)
                    break
            elif (goals[l][idx] if goals is not None else (j == 0 or i == 0 or j == H - 1 or i == W - 1)):
                goal = (l, idx)
                break
            ml = mult[l]
            for dj, di, c in nb:
                jj, ii = j + dj, i + di
                if jj < 0 or ii < 0 or jj >= H or ii >= W:
                    continue
                n = jj * W + ii
                if not ok[l][n]:
                    continue
                ng = g + c * ml[n]
                if ng < dist[l].get(n, INF):
                    dist[l][n] = ng
                    prev[(l, n)] = (l, idx)
                    heapq.heappush(heap, (ng + hfun(jj, ii), ng, l, n))
            if viaok[idx]:
                l2 = 1 - l
                if ok[l2][idx]:
                    ng = g + via_cost + vmult[idx] + mult[l2][idx]
                    if ng < dist[l2].get(idx, INF):
                        dist[l2][idx] = ng
                        prev[(l2, idx)] = (l, idx)
                        heapq.heappush(heap, (ng + hfun(j, i), ng, l2, idx))
        if goal is None:
            return None
        path = [goal]
        while path[-1] in prev:
            path.append(prev[path[-1]])
        path.reverse()
        cells, vias = [], []
        for k, (l, idx) in enumerate(path):
            j, i = divmod(idx, W)
            cells.append((l, j, i))
            if k > 0 and path[k - 1][1] == idx and path[k - 1][0] != l:
                vias.append((j, i))
        if plane:
            j, i = divmod(goal[1], W)
            if (j, i) not in vias:
                vias.append((j, i))   # the plane tap
        return dict(cells=cells, vias=vias)


def exit_sizes(L, R, todo, planes, min_size=60):
    """For each non-plane pad: how many router nodes can be reached on the end layer from the end of its escape,
    walking the router's node grid with foreign pads and the other nets' escapes as obstacles. Router-frame only."""
    from scipy import ndimage
    if not L.grid:
        return {}
    sub = L.sub
    ii = np.arange(L.W); jj = np.arange(L.H)
    ci = ii[(ii + L.ax0) % sub == 0]; cj = jj[(jj + L.ay0) % sub == 0]
    ni = (ci + L.ax0) // sub; nj = (cj + L.ay0) // sub
    key = lambda p: (p.ref, p.num)
    out = {}
    cache = {}
    for p in todo:
        if p.net in planes:
            continue
        rt = R.route.get(key(p))
        if not rt:
            continue
        l, j, i = rt['cells'][-1]
        if p.net not in cache:
            q = L.netid.get(p.net, 0)
            maps = []
            otz, ovz = R.own(p.net)
            for ll in range(2):
                f = np.where(L.gn1[ll] == q, L.gd2[ll], L.gd1[ll])
                free = f >= L.need_track
                tzf = (R.tz[ll] - otz[ll]) == 0
                lat = tzf[np.ix_(cj, ci)]
                sub_free = free[np.ix_(nj, ni)] & lat
                free = free.copy()
                free[np.ix_(nj, ni)] = sub_free
                lab, n = ndimage.label(free)
                maps.append((lab, np.bincount(lab.ravel())))
            cache[p.net] = maps
        lab, sizes = cache[p.net][l]
        a, b = (i + L.ax0) // sub, (j + L.ay0) // sub
        if b < 0 or a < 0 or b >= lab.shape[0] or a >= lab.shape[1] or lab[b, a] == 0:
            out[key(p)] = (0, l, j, i)
        else:
            out[key(p)] = (int(sizes[lab[b, a]]), l, j, i)
    return out


def solve(L, pads_to_route, planes, iters=60, log=print, hist_inc=0.4, seed=1):
    import random
    rnd = random.Random(seed)
    R = Router(L)
    order = sorted(pads_to_route, key=lambda p: (p.net in planes, p.num))
    pres = 0.4
    key = lambda p: (p.ref, p.num)
    for it in range(iters):
        failed = []
        if it > 0:
            rnd.shuffle(order)
        for p in order:
            rt = R.route.pop(key(p), None)
            if rt:
                R.stamp_route(rt, -1, p.net)
            new = R.search(p, p.net in planes, pres)
            if new is None:
                failed.append(p)
                continue
            R.route[key(p)] = new
            R.stamp_route(new, +1, p.net)
        conf = 0
        bad = 0
        for p in order:
            rt = R.route.get(key(p))
            if not rt:
                continue
            c = R.conflicts(rt, p.net)
            conf += c
            if c:
                bad += 1
                for (l, j, i) in rt['cells']:
                    if R.tz[l][j, i] - R.own(p.net)[0][l][j, i] > 0:
                        R.hist[l][max(0, j - 2):j + 3, max(0, i - 2):i + 3] += hist_inc
        badnames = ' '.join(f'{p.net}:{p.num}' for p in order if R.route.get(key(p)) and R.conflicts(R.route[key(p)], p.net))
        log(f'  iteration {it + 1}: routed {len(R.route)}/{len(order)}, failed searches {len(failed)}, nets in conflict {bad}, conflicting cells {conf}, pres {pres:.2f} [{badnames}]')
        sys.stdout.flush()
        if not failed and conf == 0:
            ex = exit_sizes(L, R, pads_to_route, planes)
            weak = {k: v for k, v in ex.items() if v[0] < 60}
            if not weak:
                return R, True
            log(f'    no conflicts, but {len(weak)} escape(s) end where the router cannot leave: ' + ' '.join(f'{k[1]}:{v[0]}' for k, v in sorted(weak.items())))
            for k, (sz, l, j, i) in weak.items():
                for ll in range(2):
                    R.hist[ll][max(0, j - 8):j + 9, max(0, i - 8):i + 9] += 3.0 * hist_inc
        pres = min(pres * 1.35, 50.0)
    return R, False


# ---------- output ---------------------------------------------------------
def write_board(src, dst, L, R, nets):
    import uuid
    code = {n: c for c, n in nets.items()}
    r = L.r
    text = open(src, encoding='utf8').read()
    out = []
    lay = lambda l: 'F.Cu' if l == 0 else 'B.Cu'
    pos = lambda j, i: (L.x0 + i * L.h, L.y0 + j * L.h)
    byname = {(p.ref, p.num): p for p in L.part}
    nsegs = nvias = 0
    placed_vias = set()
    for key, rt in sorted(R.route.items()):
        net = byname[key].net
        c = code.get(net)
        if c is None:
            continue
        cells = rt['cells']
        k = 0
        while k + 1 < len(cells):
            l, j, i = cells[k]
            l2, j2, i2 = cells[k + 1]
            if l == l2 and (j, i) != (j2, i2):
                dj, di = j2 - j, i2 - i
                e = k + 1
                while e + 1 < len(cells) and cells[e + 1][0] == l and (cells[e + 1][1] - cells[e][1], cells[e + 1][2] - cells[e][2]) == (dj, di):
                    e += 1
                (x1, y1), (x2, y2) = pos(j, i), pos(cells[e][1], cells[e][2])
                u = uuid.uuid5(uuid.NAMESPACE_URL, f'seg{key}{k}')
                out.append(f'\t(segment\n\t\t(start {x1:.4f} {y1:.4f})\n\t\t(end {x2:.4f} {y2:.4f})\n\t\t(width {r["track"]})\n\t\t(layer "{lay(l)}")\n\t\t(net {c})\n\t\t(uuid "{u}")\n\t)\n')
                nsegs += 1
                k = e
            else:
                k += 1
        for (j, i) in rt['vias']:
            if (c, j, i) in placed_vias:
                continue
            placed_vias.add((c, j, i))
            x, y = pos(j, i)
            u = uuid.uuid5(uuid.NAMESPACE_URL, f'via{key}{j},{i}')
            out.append(f'\t(via\n\t\t(at {x:.4f} {y:.4f})\n\t\t(size {r["via"]})\n\t\t(drill {r["drill"]})\n\t\t(layers "F.Cu" "B.Cu")\n\t\t(net {c})\n\t\t(uuid "{u}")\n\t)\n')
            nvias += 1
    end = text.rindex(')')
    open(dst, 'w', encoding='utf8').write(text[:end] + ''.join(out) + text[end:])
    return nsegs, nvias


def conflict_report(L, R, todo):
    """Pairs of routes of different nets that come nearer than the rules allow."""
    from scipy.spatial import cKDTree
    key = lambda p: (p.ref, p.num)
    byk = {key(p): p for p in todo}
    pts = {}
    for k, rt in R.route.items():
        pts[k] = ([(l, j, i) for (l, j, i) in rt['cells']], rt['vias'])
    out = []
    ks = sorted(pts)
    for a in range(len(ks)):
        for b in range(a + 1, len(ks)):
            ka, kb = ks[a], ks[b]
            if byk[ka].net == byk[kb].net:
                continue
            worst = None
            for l in range(2):
                A = np.array([(j, i) for (ll, j, i) in pts[ka][0] if ll == l], dtype=float)
                B = np.array([(j, i) for (ll, j, i) in pts[kb][0] if ll == l], dtype=float)
                if len(A) and len(B):
                    d, ia = cKDTree(B).query(A)
                    m = int(np.argmin(d)); dd = d[m] * L.h
                    if dd < L.tt and (worst is None or dd < worst[0]):
                        worst = (dd, 'track-track', l, A[m])
            for (kx, ky, kind, need) in ((ka, kb, 'via-track', L.vt), (kb, ka, 'via-track', L.vt)):
                for (j, i) in pts[kx][1]:
                    for l in range(2):
                        B = np.array([(jj, ii) for (ll, jj, ii) in pts[ky][0] if ll == l], dtype=float)
                        if len(B):
                            d = np.hypot(B[:, 0] - j, B[:, 1] - i).min() * L.h
                            if d < need and (worst is None or d < worst[0]):
                                worst = (d, kind, l, np.array([j, i]))
            for (j, i) in pts[ka][1]:
                for (jj, ii) in pts[kb][1]:
                    d = math.hypot(j - jj, i - ii) * L.h
                    if d < L.vv and (worst is None or d < worst[0]):
                        worst = (d, 'via-via', 0, np.array([j, i]))
            if worst:
                dd, kind, l, at = worst
                out.append(f'    {byk[ka].net}:{ka[1]} vs {byk[kb].net}:{kb[1]}  {kind} {dd:.3f} mm (rule {L.tt if kind == "track-track" else L.vt if kind == "via-track" else L.vv:.3f}) near ({L.x0 + at[1] * L.h:.2f}, {L.y0 + at[0] * L.h:.2f}) layer {l}')
    return out


def main():
    args = sys.argv[1:]
    if len(args) < 2:
        sys.exit(__doc__)
    pcb, ref = args[0], args[1]
    margin, step, planes, outp, iters, disc, hinc, drop, slack, gorigin, sub = 3.5, 0.05, [], None, 60, 0.03, 0.4, [], 0.005, None, 6
    i = 2
    while i < len(args):
        if args[i] == '--margin':
            margin = float(args[i + 1]); i += 2
        elif args[i] == '--step':
            step = float(args[i + 1]); i += 2
        elif args[i] == '--plane':
            planes.append(args[i + 1]); i += 2
        elif args[i] == '--out':
            outp = args[i + 1]; i += 2
        elif args[i] == '--grid':
            gorigin = (float(args[i + 1]), float(args[i + 2])); i += 3
        elif args[i] == '--sub':
            sub = int(args[i + 1]); i += 2
        elif args[i] == '--slack':
            slack = float(args[i + 1]); i += 2
        elif args[i] == '--drop':
            drop.append(args[i + 1]); i += 2
        elif args[i] == '--disc':
            disc = float(args[i + 1]); i += 2
        elif args[i] == '--hist':
            hinc = float(args[i + 1]); i += 2
        elif args[i] == '--iters':
            iters = int(args[i + 1]); i += 2
        else:
            sys.exit('unknown ' + args[i])
    root, nets, pads = load_pads(pcb)
    if drop:
        print('WHAT-IF: ignoring pads of', ', '.join(drop))
        pads = [p for p in pads if p.ref not in drop]
    rules = load_rules(pcb)
    print('rules', rules)
    grid = None
    if gorigin:
        grid = (gorigin, rules['track'] + rules['clr'] + rules['margin'])
        print('router-frame lattice: origin', gorigin, 'grid pitch', round(grid[1], 4), 'sub', sub)
    L = Lattice(pads, ref, rules, margin, step, planes, disc, slack, grid, sub)
    todo = [p for p in L.part if p.net]
    print(f'{ref}: {len(todo)} pads with a net ({sum(1 for p in todo if p.net in L.planes)} on plane nets), window {L.W}x{L.H} cells of {step} mm')
    R, ok = solve(L, todo, L.planes, iters, hist_inc=hinc)
    print('RESULT:', 'every pad escapes with no overlap' if ok else 'NOT solved in the iteration limit')
    for line in conflict_report(L, R, todo):
        print(line)
    if outp:
        s, v = write_board(pcb, outp, L, R, nets)
        print(f'wrote {outp}: {s} segments, {v} vias')


if __name__ == '__main__':
    main()
