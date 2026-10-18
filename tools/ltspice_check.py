"""Per-board LTspice power-stage check for the converted AP63203 regulators.

    python tools/ltspice_check.py BOARD_ID NETLIST_DIR CORPUSFB_DIR [--isat 5.5] [--margin 0.8]

For each regulator in CORPUSFB_DIR/fbfix.json (the edited netlist under CORPUSFB_DIR/NETLIST_DIR):
  - SW net = regulator pad 5 net, output net = the net its pad 1 now sits on, input net = pad 3 net;
  - inductor = the component on the SW net whose other pin is on the output net (value from the netlist);
  - output capacitance = sum of every capacitor on the output net (netlist values); a part over 1 mF
    (the supercap) is left out and named;
then rewrites the `.param lval=... ctot=...` line of tools/ltspice/ap63203_stage.cir, runs LTspice in batch mode
(LTSPICE_EXE or %LOCALAPPDATA%/Programs/ADI/LTspice/LTspice.exe) in out/ltspice/BOARD_ID_REF/, and parses the
.meas tables: worst inductor peak current and worst output ripple over Vin 9/12/24/30 V and capacitance at
100/50/35 % (DC-bias derating, an assumption). PASS needs peak current <= margin x Isat (Isat of the Sunlord
SWPA6045S4R7MT in the order BOM, 5.5 A). The model is open loop with assumed MOSFET resistances: it checks the
power stage, not the IC's control loop, start-up or current limit (see the header of the .cir).
"""

import glob
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fb_fix  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
EXE = os.environ.get("LTSPICE_EXE", os.path.join(os.environ.get("LOCALAPPDATA", ""), "Programs", "ADI", "LTspice", "LTspice.exe"))
VINS, DERATES = [9, 12, 24, 30], [1, 0.5, 0.35]


def val(s):
    m = re.fullmatch(r"\s*([0-9.]+)\s*([pnuµmkKM]?)\s*([FHfh]?)\s*", s.replace("μ", "u"))
    if not m:
        return None
    return float(m.group(1)) * {"": 1, "p": 1e-12, "n": 1e-9, "u": 1e-6, "µ": 1e-6, "m": 1e-3, "k": 1e3, "K": 1e3, "M": 1e6}[m.group(2)]


def parse_net(t):
    comps = {}
    for m in re.finditer(r'\(comp\s+\(ref "([^"]*)"\)\s+\(value "([^"]*)"\)', t):
        blk = t[m.start():fb_fix.match_paren(t, m.start())]
        fp = re.search(r'\(footprint "([^"]*)"\)', blk)
        comps[m.group(1)] = (m.group(2), fp.group(1) if fp else "")
    nets = {}
    for m in re.finditer(r'\(net\s+\(code \d+\)\s+\(name "([^"]*)"\)', t):
        blk = t[m.start():fb_fix.match_paren(t, m.start())]
        nets[m.group(1)] = {(a, b) for a, b in re.findall(r'\(node\s+\(ref "([^"]*)"\)\s+\(pin "([^"]*)"\)', blk)}
    return comps, nets


def tables(log):
    """{measure name: [value per step]} from an LTspice .log."""
    out, cur = {}, None
    for line in log.splitlines():
        m = re.match(r"Measurement: (\w+)", line)
        if m:
            cur = m.group(1)
            out[cur] = []
        elif cur and re.match(r"\s+\d+\s", line):
            out[cur].append(float(line.split()[1]))
    return out


def main(argv):
    bid, nd, cfb = argv[:3]
    isat = float(argv[argv.index("--isat") + 1]) if "--isat" in argv else 5.5
    margin = float(argv[argv.index("--margin") + 1]) if "--margin" in argv else 0.8
    fb = json.load(open(os.path.join(cfb, "fbfix.json")))
    net_p = glob.glob(os.path.join(cfb, nd, "hardware", "kicad", "*.net"))[0]
    comps, nets = parse_net(open(net_p, encoding="utf-8", newline="").read())
    tpl = open(os.path.join(HERE, "ltspice", "ap63203_stage.cir"), encoding="utf-8").read()
    rows, allok = [], True
    for u in fb["convert"]:
        net_of = lambda pin: [n for n, s in nets.items() if (u, pin) in s][0]
        sw, vout, vin = net_of("5"), net_of("1"), net_of("3")
        ind = [r for r, p in nets[sw] if r != u and any(rr == r for rr, _ in nets[vout])]
        if len(ind) != 1:
            print(f"{bid} {u}: SKIP inductor not unique on {sw}: {ind}")
            allok = False
            continue
        lval = val(comps[ind[0]][0])
        caps = [(r, val(comps[r][0])) for r, _ in sorted(nets[vout]) if comps.get(r, ("", ""))[1].startswith("Capacitor") and val(comps[r][0])]
        caps = sorted(set(caps))
        big = [c for c in caps if c[1] > 1e-3]
        ctot = sum(v for _, v in caps if v <= 1e-3)
        cin = sum(val(comps[r][0]) or 0 for r, _ in sorted(nets[vin]) if comps.get(r, ("", ""))[1].startswith("Capacitor"))
        if lval is None or ctot <= 0:
            print(f"{bid} {u}: SKIP unreadable inductor/capacitor value")
            allok = False
            continue
        wd = os.path.join("out", "ltspice", f"{bid}_{u}")
        os.makedirs(wd, exist_ok=True)
        cir = re.sub(r"^\.param lval=.*$", f".param lval={lval:.4g} ctot={ctot:.4g}", tpl, flags=re.M)
        cp = os.path.join(wd, "stage.cir")
        open(cp, "w", encoding="utf-8").write(cir)
        subprocess.run([EXE, "-b", os.path.abspath(cp)], capture_output=True, timeout=300)
        lp = os.path.join(wd, "stage.log")
        tb = tables(open(lp, encoding="utf-8", errors="replace").read()) if os.path.exists(lp) else {}
        if not tb.get("ipk") or len(tb["ipk"]) != len(VINS) * len(DERATES):
            print(f"{bid} {u}: FAIL LTspice produced no complete tables ({wd})")
            allok = False
            continue
        i_pk, i_min, v_pp = max(tb["ipk"]), min(tb["imin"]), max(tb["vpp"])
        worst_v = tb["vpp"].index(v_pp)
        ok = i_pk <= margin * isat
        allok &= ok
        print(f"{bid} {u}: L={lval * 1e6:.2f} uH ({ind[0]}), Cout on {vout}: {ctot * 1e6:.1f} uF from {len(caps)} caps"
              f"{' (left out: ' + ', '.join(f'{r} {v}F' for r, v in big) + ')' if big else ''}, Cin on {vin}: {cin * 1e6:.1f} uF | "
              f"peak inductor current {i_pk:.2f} A at 2 A load (limit {margin * isat:.1f} A), ripple {i_pk - tb['imin'][tb['ipk'].index(i_pk)]:.2f} A p-p, "
              f"worst output ripple {v_pp * 1e3:.1f} mV p-p at Vin {VINS[worst_v % 4]} V, C x{DERATES[worst_v // 4]} | {'PASS' if ok else 'FAIL'}")
        rows.append({"board": bid, "reg": u, "L_uH": lval * 1e6, "C_uF": ctot * 1e6, "ipk_A": i_pk, "vpp_mV": v_pp * 1e3, "pass": ok})
    json.dump(rows, open(os.path.join("out", "ltspice", f"{bid}.json"), "w"), indent=1)
    return allok


if __name__ == "__main__":
    sys.exit(0 if main(sys.argv[1:]) else 1)
