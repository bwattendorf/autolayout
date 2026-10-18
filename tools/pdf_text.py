"""Extract the text of a PDF to a .txt file (PyMuPDF), one page per form-feed block.

    python tools/pdf_text.py FILE.pdf OUT.txt
"""

import sys

import fitz


def main(argv):
    if len(argv) != 2:
        sys.exit(__doc__)
    doc = fitz.open(argv[0])
    with open(argv[1], "w", encoding="utf-8") as f:
        for i, page in enumerate(doc):
            f.write(f"\n=== page {i + 1} ===\n")
            f.write(page.get_text())
    print(f"{len(doc)} pages -> {argv[1]}")


if __name__ == "__main__":
    main(sys.argv[1:])
