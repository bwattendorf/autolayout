"""Render one PDF page to a PNG (PyMuPDF), so figures can be looked at.

    python tools/pdf_page_png.py FILE.pdf PAGE_NUMBER OUT.png [ZOOM]
"""

import sys

import fitz


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    doc = fitz.open(argv[0])
    zoom = float(argv[3]) if len(argv) > 3 else 1.6
    doc[int(argv[1]) - 1].get_pixmap(matrix=fitz.Matrix(zoom, zoom)).save(argv[2])
    print(argv[2])


if __name__ == "__main__":
    main(sys.argv[1:])
