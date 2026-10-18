"""Route a placed board under a different project file's rules, quickly.

    python tools/route_with_pro.py PLACED.kicad_pcb RULES.kicad_pro [--iters 40]

Copies PLACED to a work copy beside it named `<stem>_rules.kicad_pcb` with RULES as its project file
(the router refuses to guess rules; this is how to ask "what if these rules applied"), finds the planes,
and runs `autolayout route --quick`. Prints the router's own unconnected count (NOT KiCad's).
"""

import os
import re
import shutil
import subprocess
import sys

EXE = os.environ.get("AUTOLAYOUT", os.path.join("target_dev", "release", "autolayout.exe"))


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    placed, rules = argv[0], argv[1]
    iters = argv[argv.index("--iters") + 1] if "--iters" in argv else "40"
    work = os.path.splitext(placed)[0] + "_rules.kicad_pcb"
    shutil.copyfile(placed, work)
    shutil.copyfile(rules, os.path.splitext(work)[0] + ".kicad_pro")
    code, out = run([EXE, "planes", work])
    flags = []
    for line in out.splitlines():
        if line.startswith("FLAGS\t"):
            flags = [x for x in line.split("\t")[1:] if x]
    routed = os.path.splitext(work)[0] + "_routed.kicad_pcb"
    if os.path.exists(routed):
        os.remove(routed)
    code, out = run([EXE, "route", work, *flags, "--quick", "--iters", iters, "--out", routed])
    m = re.search(r"verifier unconnected (\d+)", out)
    n = re.search(r"nets complete (\d+) of (\d+)", out)
    p = re.search(r"pitch ([\d.]+) mm", out)
    print(f"{os.path.basename(placed)} under {os.path.basename(rules)}: pitch {p.group(1) if p else '?'} mm, "
          f"router unconnected {m.group(1) if m else '?'}, nets complete {n.group(1) + '/' + n.group(2) if n else '?'}")


if __name__ == "__main__":
    main(sys.argv[1:])
