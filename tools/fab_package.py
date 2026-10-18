"""Export a routed board as a bare-PCB manufacturing package (Gerbers + drill + zip) with kicad-cli.

    python tools/fab_package.py BOARD.kicad_pcb OUT_DIR [--name NAME] [--layers "F.Cu,In1.Cu,..."]

Writes OUT_DIR/NAME/ (Gerber files, Excellon drill files with plated and unplated holes separate, a drill
map) and OUT_DIR/NAME_gerbers.zip, then lists what was produced. Uses KiCad's own exporter ($KICAD_CLI or
KiCad 10), so the files are KiCad's, not ours. The copper layers are taken from the board's stackup.
This is the bare board only: assembly files (BOM, pick-and-place) are a separate step. A DRC-clean,
fully routed board is NOT a reviewed design: read the board in KiCad before ordering (placement of power,
crystal, RF and connectors; polarity and orientation of parts; silkscreen).
"""

import os
import re
import subprocess
import sys
import zipfile

CLI = os.environ.get("KICAD_CLI", "C:/Program Files/KiCad/10.0/bin/kicad-cli.exe")


def run(cmd):
    p = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    return p.returncode, p.stdout + p.stderr


def copper_layers(pcb):
    text = open(pcb, encoding="utf-8").read()
    return re.findall(r'\(\d+ "([^"]+\.Cu)"', text)


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    pcb, out = argv[0], argv[1]
    name = argv[argv.index("--name") + 1] if "--name" in argv else os.path.splitext(os.path.basename(pcb))[0]
    layers = argv[argv.index("--layers") + 1] if "--layers" in argv else ",".join(
        copper_layers(pcb) + ["F.Paste", "B.Paste", "F.Silkscreen", "B.Silkscreen", "F.Mask", "B.Mask", "Edge.Cuts"])
    target = os.path.join(out, name)
    os.makedirs(target, exist_ok=False)
    code, text = run([CLI, "pcb", "export", "gerbers", "--output", target + os.sep, "--layers", layers, pcb])
    print(text.strip()[-300:])
    if code != 0:
        sys.exit("gerber export failed")
    code, text = run([CLI, "pcb", "export", "drill", "--output", target + os.sep, "--format", "excellon",
                      "--excellon-units", "mm", "--excellon-separate-th", "--drill-origin", "absolute",
                      "--generate-map", "--map-format", "gerberx2", pcb])
    print(text.strip()[-300:])
    if code != 0:
        sys.exit("drill export failed")
    zpath = os.path.join(out, name + "_gerbers.zip")
    with zipfile.ZipFile(zpath, "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(os.listdir(target)):
            z.write(os.path.join(target, f), f)
    files = sorted(os.listdir(target))
    print(f"{name}: {len(files)} file(s) in {target}, zipped to {zpath} ({os.path.getsize(zpath) // 1024} KiB)")
    for f in files:
        print(f"  {f}  {os.path.getsize(os.path.join(target, f))} bytes")


if __name__ == "__main__":
    main(sys.argv[1:])
