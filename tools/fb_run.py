"""Re-run boards with their 3.3 V AP63201 regulators turned into fixed AP63203s, then rebuild the order files.

    python tools/fb_run.py BOARD_ID [BOARD_ID ...] [--small ID,ID,...]

Per board: tools/fb_fix.py builds out/corpusfb_<id> from the board's latest swapped corpus (out/corpus_<id>, or
out/corpus_cap for m1, else the real corpus); tools/place_sweep.py (KiCad's Python) places and routes it into
out/fb_<id> under --rules jlc-std (jlc-small for ids in --small); tools/order_pack.py writes out/order/<id>_fb
with the AP63203 BOM lines. One board at a time; the summary line of each step is appended to out/fb_run.log.
"""

import glob
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
KPY = "C:/Program Files/KiCad/10.0/bin/python.exe"
REAL = "C:/Portcullis/controller/boards"


def log(msg):
    print(msg, flush=True)
    with open(os.path.join(ROOT, "out", "fb_run.log"), "a", encoding="utf-8") as f:
        f.write(msg + "\n")


def netdir(bid):
    for d in sorted(glob.glob(os.path.join(REAL, bid + "-*"))):
        if glob.glob(os.path.join(d, "hardware", "kicad", "*.net")):
            return os.path.basename(d)
    return None


def run(cmd, env=None, tail=1):
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", cwd=ROOT, env=env)
    lines = (p.stdout + p.stderr).strip().splitlines()
    return p.returncode, "\n".join(lines[-tail:])


def main(argv):
    small = []
    if "--small" in argv:
        small = argv[argv.index("--small") + 1].split(",")
        argv = [a for a in argv if a not in ("--small", ",".join(small))]
    for bid in argv:
        nd = netdir(bid)
        src = REAL
        for cand in (f"out/corpus_{bid}", "out/corpus_cap" if bid == "m1" else ""):
            if cand and os.path.exists(os.path.join(ROOT, cand, "quilter", "freeplace", bid)):
                src = os.path.join(ROOT, cand)
                break
        corp = os.path.join(ROOT, "out", f"corpusfb_{bid}")
        code, out = run([sys.executable, os.path.join(HERE, "fb_fix.py"), bid, nd, corp, "--from-corpus", src], tail=4)
        log(f"{bid}: fb_fix from {os.path.relpath(src, ROOT) if src != REAL else 'real corpus'}: {out.replace(chr(10), ' | ')}")
        if code or not os.path.exists(os.path.join(corp, "fbfix.json")):
            log(f"{bid}: no conversion or fb_fix failed, skipped")
            continue
        sweep = os.path.join(ROOT, "out", f"fb_{bid}")
        env = dict(os.environ, AUTOLAYOUT_CORPUS=corp.replace("\\", "/"))
        rules = "jlc-small" if bid in small else "jlc-std"
        code, out = run([KPY, os.path.join(HERE, "place_sweep.py"), sweep, "--rules", rules, "--boards", bid, "--check"], env=env, tail=3)
        log(f"{bid}: place_sweep {rules} exit={code}: {out.replace(chr(10), ' | ')}")
        if code:
            continue
        extra = ["--set", "AP63201=C2158003"] if bid == "m1" else []
        code, out = run([sys.executable, os.path.join(HERE, "order_pack.py"), bid, sweep, os.path.join(ROOT, "out", "order", bid + "_fb"),
                         "--fbfix", os.path.join(corp, "fbfix.json"), *extra], tail=1)
        log(f"{bid}: order_pack exit={code}: {out}")


if __name__ == "__main__":
    main(sys.argv[1:])
