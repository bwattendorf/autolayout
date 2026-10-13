"""KiCad-flip every footprint on a board about its own position and save.
The reference for verifying `autolayout flip ALL` (compare with
tools/kicad_pads.py + tools/compare_pads.sh). Public pcbnew API only.

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/kicad_flip_all.py IN.kicad_pcb OUT.kicad_pcb
"""
import sys

import pcbnew

if len(sys.argv) != 3:
    sys.exit(__doc__)
board = pcbnew.LoadBoard(sys.argv[1])
n = 0
for fp in board.GetFootprints():
    fp.Flip(fp.GetPosition(), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)
    n += 1
board.Save(sys.argv[2])
print(f"KiCad flipped {n} footprints -> {sys.argv[2]}")
