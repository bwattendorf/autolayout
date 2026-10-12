"""Every pad's board position as KiCad itself computes it.

Run under KiCad's bundled Python (it needs `pcbnew`):

    "C:/Program Files/KiCad/9.0/bin/python.exe" tools/kicad_pads.py board.kicad_pcb

Prints the same tab-separated `REF PAD X Y ANGLE` rows, sorted, as
`autolayout pads`, so the two can be diffed. This is the external grader for
the board model and its edits (CLAUDE.md rule 3): autolayout computes pad
positions from the file; KiCad computes them from the same file; they must
agree. Uses only the public pcbnew API; no KiCad source is copied.
"""

import sys

import pcbnew


def main(path):
    board = pcbnew.LoadBoard(path)
    rows = []
    for fp in board.GetFootprints():
        ref = fp.GetReference()
        for pad in fp.Pads():
            pos = pad.GetPosition()
            angle = pad.GetOrientation().AsDegrees() % 360.0
            rows.append("%s\t%s\t%.4f\t%.4f\t%.1f" % (
                ref, pad.GetNumber(),
                pcbnew.ToMM(pos.x), pcbnew.ToMM(pos.y), angle))
    rows.sort()
    sys.stdout.write("\n".join(rows) + "\n")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: kicad_pads.py <file.kicad_pcb>")
    main(sys.argv[1])
