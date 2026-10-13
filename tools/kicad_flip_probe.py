"""Have KiCad flip footprints, so autolayout's flip can be derived from what
KiCad actually writes rather than from a guess.

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/kicad_flip_probe.py IN.kicad_pcb OUT.kicad_pcb REF [REF ...]

Flips each named footprint about its own position, left-right (KiCad's
default flip direction in pcbnew), and saves to OUT. Diff the footprint blocks
of IN and OUT to see every token KiCad changes. Public pcbnew API only.
"""

import sys

import pcbnew


def main(src, dst, refs):
    board = pcbnew.LoadBoard(src)
    for ref in refs:
        fp = board.FindFootprintByReference(ref)
        if fp is None:
            sys.exit(f"no footprint {ref}")
        fp.Flip(fp.GetPosition(), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)
    board.Save(dst)
    print(f"flipped {' '.join(refs)} -> {dst}")


if __name__ == "__main__":
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    main(sys.argv[1], sys.argv[2], sys.argv[3:])
