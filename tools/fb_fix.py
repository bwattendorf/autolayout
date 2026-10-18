"""Turn adjustable AP63201 buck regulators that make 3.3 V into the fixed-output AP63203 (netlist edit on a copy).

    python tools/fb_fix.py BOARD_ID NETLIST_DIR OUT_CORPUS [--from-corpus DIR] [--mpn AP63201WU-7] [--vref 0.8]
                           [--vout 3.3] [--tol 0.15] [--scan]

For every BOM line with `--mpn` (designators from $AUTOLAYOUT_CORPUS/_bom/<id>_jlc_bom.csv) it reads pad 1 (FB)
and pad 4 (ground) of the regulator on the copy of the board, finds the two resistors on the FB net, takes the
one whose other pad is on the ground-pin net as the bottom resistor, and computes Vout = vref * (1 + Rtop/Rbot).
Regulators within `--tol` of `--vout` are converted: both divider resistors are deleted and pad 1 is joined to
the net on the top resistor's other pad (the Diodes AP63203 evaluation guide says to connect VFB straight to the
output: https://www.diodes.com/assets/Evaluation-Boards/AP63203-EVB-User-Guide.pdf). The same edit is made in
the netlist file. Anything else on the FB net, or a non-matching voltage, leaves that regulator alone and says why.

Writes OUT_CORPUS (a one-board corpus like tools/cap_fix.py) plus OUT_CORPUS/fbfix.json:
{"convert": [regulator refs], "remove": [resistor refs]} for the order BOM. The inputs are never modified.
`--scan` prints the analysis and writes nothing. The pad functions (1 FB, 4 GND) and vref come from the
AP6320x family datasheet as recalled; the printed m1 result (3.33 V) is the check on vref.
"""

import csv
import glob
import json
import os
import re
import shutil
import sys

BOM = "C:/Portcullis/controller/boards/_bom"


def match_paren(t, i):
    """Index just past the ')' that closes the '(' at t[i]; quotes respected."""
    depth, q = 0, False
    while i < len(t):
        c = t[i]
        if q:
            if c == "\\":
                i += 1
            elif c == '"':
                q = False
        elif c == '"':
            q = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    raise ValueError("unbalanced")


def footprints(t):
    """[(start, end, ref, value, {pad: (netcode, netname)})] for the board text."""
    out = []
    for m in re.finditer(r'\n\t\(footprint "', t):
        s = m.start() + 1
        e = match_paren(t, s)
        blk = t[s:e]
        ref = re.search(r'\(property "Reference" "([^"]*)"', blk)
        val = re.search(r'\(property "Value" "([^"]*)"', blk)
        pads = {}
        for pm in re.finditer(r'\(pad "([^"]*)"', blk):
            pe = match_paren(blk, pm.start())
            nm = re.search(r'\(net (\d+) "([^"]*)"\)', blk[pm.start():pe])
            if nm:
                pads[pm.group(1)] = (nm.group(1), nm.group(2))
        out.append((s, e, ref.group(1) if ref else "", val.group(1) if val else "", pads))
    return out


def ohms(v):
    m = re.fullmatch(r"([0-9.]+)\s*([kKmMRr]?)", v.strip())
    if not m:
        return None
    return float(m.group(1)) * {"": 1, "r": 1, "R": 1, "k": 1e3, "K": 1e3, "m": 1e-3, "M": 1e6}[m.group(2)]


def analyse(pcb, refs, vref):
    fps = footprints(pcb)
    by_ref = {f[2]: f for f in fps}
    res = []
    for u in refs:
        f = by_ref.get(u)
        if not f or "1" not in f[4] or "4" not in f[4]:
            res.append({"ref": u, "ok": False, "why": "regulator or pad 1/4 not found"})
            continue
        fb, gnd = f[4]["1"], f[4]["4"]
        others = [(g[2], p) for g in fps for p, n in g[4].items() if n == fb and not (g[2] == u and p == "1")]
        rs = [(r, p) for r, p in others if r in by_ref and ohms(by_ref[r][3]) is not None and len(by_ref[r][4]) == 2]
        if len(others) != 2 or len(rs) != 2 or rs[0][0] == rs[1][0]:
            res.append({"ref": u, "ok": False, "why": f"FB net has {others}, not exactly two resistors"})
            continue
        bot = [x for x in rs if [n for q, n in by_ref[x[0]][4].items() if q != x[1]][0] == gnd]
        if len(bot) != 1:
            res.append({"ref": u, "ok": False, "why": "no single resistor from FB to the ground-pin net"})
            continue
        top = [x for x in rs if x != bot[0]][0]
        vout_net = [n for q, n in by_ref[top[0]][4].items() if q != top[1]][0]
        rt, rb = ohms(by_ref[top[0]][3]), ohms(by_ref[bot[0][0]][3])
        res.append({"ref": u, "ok": True, "top": top[0], "bot": bot[0][0], "rtop": rt, "rbot": rb,
                    "vout": round(vref * (1 + rt / rb), 3), "vout_net": vout_net[1],
                    "vout_code": vout_net[0], "fb_net": fb[1]})
    return res


def drop_block(t, head_re):
    m = re.search(head_re, t)
    if not m:
        return t, False
    s = m.start()
    e = match_paren(t, s)
    ls = t.rfind("\n", 0, s) + 1
    le = t.find("\n", e - 1)
    le = len(t) if le < 0 else le + 1
    if t[ls:s].strip():
        ls = s
    return t[:ls] + t[le:], True


def edit_pcb(pcb, conv):
    for c in conv:
        fps = {f[2]: f for f in footprints(pcb)}
        for r in (c["top"], c["bot"]):
            s, e = fps[r][0], fps[r][1]
            ls = pcb.rfind("\n", 0, s) + 1
            pcb = pcb[:ls] + pcb[pcb.find("\n", e) + 1:]
            fps = {f[2]: f for f in footprints(pcb)}
        s, e, _, _, pads = fps[c["ref"]]
        blk = pcb[s:e]
        pm = re.search(r'\(pad "1"', blk)
        pe = match_paren(blk, pm.start())
        pad = blk[pm.start():pe]
        new = re.sub(r'\(net (\d+) "[^"]*"\)', f'(net {c["vout_code"]} "{c["vout_net"]}")', pad, count=1)
        pcb = pcb[:s] + blk[:pm.start()] + new + blk[pe:] + pcb[e:]
    return pcb


def net_code(pcb, name):
    m = re.search(r'\n\t\(net (\d+) "%s"\)' % re.escape(name), pcb)
    return m.group(1) if m else None


def edit_net(net, conv):
    for c in conv:
        for r in (c["top"], c["bot"]):
            net, ok = drop_block(net, r'\(comp\s+\(ref "%s"\)' % re.escape(r))
            if not ok:
                raise ValueError(f"netlist has no comp {r}")
            while True:
                net, ok = drop_block(net, r'\(node\s+\(ref "%s"\)' % re.escape(r))
                if not ok:
                    break
        m = re.search(r'\(net\s+\(code \d+\)\s+\(name "%s"\)' % re.escape(c["fb_net"]), net)
        if not m:
            raise ValueError("netlist has no FB net " + c["fb_net"])
        s = m.start()
        e = match_paren(net, s)
        blk = net[s:e]
        nm = re.search(r'\(node\s+\(ref "%s"\)\s+\(pin "1"\)' % re.escape(c["ref"]), blk)
        node = blk[nm.start():match_paren(blk, nm.start())]
        net, _ = drop_block(net, r'\(net\s+\(code \d+\)\s+\(name "%s"\)' % re.escape(c["fb_net"]))
        m2 = re.search(r'\(net\s+\(code \d+\)\s+\(name "%s"\)' % re.escape(c["vout_net"]), net)
        if not m2:
            raise ValueError("netlist has no output net " + c["vout_net"])
        e2 = match_paren(net, m2.start())
        net = net[:e2 - 1] + "\n      " + node + ")" + net[e2:]
    return net


def main(argv):
    if len(argv) < 3 and "--scan" not in argv:
        sys.exit(__doc__)
    opt = lambda k, d: argv[argv.index(k) + 1] if k in argv else d
    bid, net_dir = argv[0], argv[1]
    out = argv[2] if len(argv) > 2 and not argv[2].startswith("--") else None
    real = os.environ.get("AUTOLAYOUT_CORPUS", "C:/Portcullis/controller/boards")
    src = opt("--from-corpus", real)
    mpn, vref, vout, tol = opt("--mpn", "AP63201WU-7"), float(opt("--vref", "0.8")), float(opt("--vout", "3.3")), float(opt("--tol", "0.15"))
    refs = []
    with open(f"{BOM}/{bid}_jlc_bom.csv", encoding="utf-8-sig", newline="") as f:
        for r in csv.DictReader(f):
            if r.get("MPN (for search)", "").strip() == mpn:
                refs += [d.strip() for d in r["Designator"].split(",") if d.strip()]
    pcb_in = os.path.join(src, "quilter", "freeplace", bid, bid + ".kicad_pcb")
    pcb = open(pcb_in, encoding="utf-8", newline="").read()
    an = analyse(pcb, refs, vref)
    conv = []
    for a in an:
        if not a["ok"]:
            print(f"{bid} {a['ref']}: SKIP {a['why']}")
        elif abs(a["vout"] - vout) <= tol:
            print(f"{bid} {a['ref']}: {a['rtop']:.0f}/{a['rbot']:.0f} ohm -> {a['vout']} V on {a['vout_net']}: convert, drop {a['top']} {a['bot']}")
            conv.append(a)
        else:
            print(f"{bid} {a['ref']}: {a['rtop']:.0f}/{a['rbot']:.0f} ohm -> {a['vout']} V: not {vout} V, left as AP63201")
    if "--scan" in argv or not conv:
        return
    nets = sorted(glob.glob(os.path.join(src, net_dir, "hardware", "kicad", "*.net")))
    if len(nets) != 1:
        sys.exit(f"expected one netlist in {src}/{net_dir}")
    newpcb = edit_pcb(pcb, conv)
    newnet = edit_net(open(nets[0], encoding="utf-8", newline="").read(), conv)
    if os.path.abspath(out) == os.path.abspath(src):
        sys.exit("refusing to write over the source corpus")
    fp = os.path.join(out, "quilter", "freeplace", bid)
    os.makedirs(fp, exist_ok=True)
    shutil.copy(os.path.join(real, "quilter", "freeplace", bid, bid + ".kicad_pro"), fp)
    nd = os.path.join(out, net_dir, "hardware", "kicad")
    os.makedirs(nd, exist_ok=True)
    open(os.path.join(fp, bid + ".kicad_pcb"), "w", encoding="utf-8", newline="").write(newpcb)
    open(os.path.join(nd, os.path.basename(nets[0])), "w", encoding="utf-8", newline="").write(newnet)
    for d in glob.glob(os.path.join(real, "quilter", "returned", bid, "3p5mil-candidate*")):
        dd = os.path.join(out, "quilter", "returned", bid, os.path.basename(d))
        os.makedirs(dd, exist_ok=True)
        shutil.copy(os.path.join(d, bid + ".kicad_pro"), dd)
    json.dump({"convert": [c["ref"] for c in conv], "remove": [r for c in conv for r in (c["top"], c["bot"])]},
              open(os.path.join(out, "fbfix.json"), "w"))
    print(f"{bid}: wrote {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
