"""Independent check of tools/fb_fix.py output: rebuild the expected connectivity from the ORIGINAL netlist
and compare it with the edited netlist and the edited board.

    python tools/fb_verify.py BOARD_ID NETLIST_DIR CORPUSFB_DIR [--from-corpus DIR]

Checks (each prints PASS/FAIL and the evidence):
 1. netlist: applying "delete the listed resistors, move each converted regulator's pad 1 onto the net its top
    resistor fed" to the original netlist gives exactly the edited netlist (as sets of (ref, pin) per net).
 2. board vs netlist: the edited board's pad groups equal the edited netlist's, for pads present in both, and
    the mismatch count is no worse than the original board vs the original netlist (r and m2 are stale).
 3. nothing else changed: the board's footprint list differs from the original by exactly the removed refs.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fb_fix  # noqa: E402

REAL = "C:/Portcullis/controller/boards"


def read(p):
    return open(p, encoding="utf-8", newline="").read()


def nets_of_netlist(t):
    out = {}
    for m in re.finditer(r'\(net\s+\(code \d+\)\s+\(name "([^"]*)"\)', t):
        blk = t[m.start():fb_fix.match_paren(t, m.start())]
        out[m.group(1)] = {(a, b) for a, b in re.findall(r'\(node\s+\(ref "([^"]*)"\)\s+\(pin "([^"]*)"\)', blk)}
    return out


def comps_of_netlist(t):
    return set(re.findall(r'\(comp\s+\(ref "([^"]*)"\)', t))


def groups(d):
    return {frozenset(v) for v in d.values() if v}


def board_nets(pcb):
    d = {}
    for _, _, ref, _, pads in fb_fix.footprints(pcb):
        for p, (_, name) in pads.items():
            if name:
                d.setdefault(name, set()).add((ref, p))
    return d


def mismatches(board, net):
    """Pads present in both whose net group differs; returns count."""
    pos = {p for g in net.values() for p in g}
    bpos = {p for g in board.values() for p in g}
    both = pos & bpos
    nb = {frozenset(g & both) for g in board.values() if g & both}
    nn = {frozenset(g & both) for g in net.values() if g & both}
    return len(nb ^ nn)


def main(argv):
    bid, nd, cfb = argv[:3]
    src = argv[argv.index("--from-corpus") + 1] if "--from-corpus" in argv else REAL
    fb = json.load(open(os.path.join(cfb, "fbfix.json")))
    old_net_p = fb_fix.glob.glob(os.path.join(REAL, nd, "hardware", "kicad", "*.net"))[0]
    new_net_p = fb_fix.glob.glob(os.path.join(cfb, nd, "hardware", "kicad", "*.net"))[0]
    old_net, new_net = read(old_net_p), read(new_net_p)
    old_pcb = read(os.path.join(src, "quilter", "freeplace", bid, bid + ".kicad_pcb"))
    new_pcb = read(os.path.join(cfb, "quilter", "freeplace", bid, bid + ".kicad_pcb"))

    # 1. expected netlist from the original
    on = nets_of_netlist(old_net)
    pairs = []
    for u in fb["convert"]:
        fbnet = [n for n, s in on.items() if (u, "1") in s][0]
        rs = [r for r in fb["remove"] if any((r, p) in on[fbnet] for p in ("1", "2"))]
        # the top resistor is the removed one NOT touching the ground-pin net
        gnd = [n for n, s in on.items() if (u, "4") in s][0]
        top = [r for r in rs if not any((r, p) in on[gnd] for p in ("1", "2"))][0]
        vnet = [n for n, s in on.items() if n != fbnet and any((top, p) in s for p in ("1", "2"))][0]
        pairs.append((u, fbnet, vnet))
    exp = {n: set(s) for n, s in on.items()}
    for u, fbnet, vnet in pairs:
        exp[fbnet].discard((u, "1"))
        exp[vnet].add((u, "1"))
    for n in exp:
        exp[n] = {x for x in exp[n] if x[0] not in fb["remove"]}
    got = nets_of_netlist(new_net)
    ok1 = groups(exp) == groups(got) and comps_of_netlist(old_net) - set(fb["remove"]) == comps_of_netlist(new_net)
    print(f"{bid} 1 netlist matches expected edit: {'PASS' if ok1 else 'FAIL'} "
          f"(regulators {[p[0] for p in pairs]}, joined to {[p[2] for p in pairs]}, removed {fb['remove']})")
    if not ok1:
        a, b = groups(exp) - groups(got), groups(got) - groups(exp)
        print("   expected-only:", [sorted(x)[:6] for x in a][:3], "got-only:", [sorted(x)[:6] for x in b][:3])

    # 2. board vs netlist
    m_old = mismatches(board_nets(old_pcb), nets_of_netlist(old_net))
    m_new = mismatches(board_nets(new_pcb), got)
    print(f"{bid} 2 board-vs-netlist differing groups: original {m_old}, edited {m_new}: {'PASS' if m_new <= m_old else 'FAIL'}")

    # 3. footprints
    fo = [f[2] for f in fb_fix.footprints(old_pcb)]
    fn = [f[2] for f in fb_fix.footprints(new_pcb)]
    ok3 = sorted(set(fo) - set(fn)) == sorted(fb["remove"]) and not (set(fn) - set(fo)) and len(fo) - len(fn) == len(fb["remove"])
    print(f"{bid} 3 footprints removed exactly {fb['remove']}: {'PASS' if ok3 else 'FAIL'} ({len(fo)} -> {len(fn)})")
    return ok1 and m_new <= m_old and ok3


if __name__ == "__main__":
    sys.exit(0 if main(sys.argv[1:]) else 1)
