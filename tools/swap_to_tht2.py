"""Replace one two-pad footprint in a board with a two-pad through-hole footprint, keeping nets and position.

    python tools/swap_to_tht2.py IN.kicad_pcb OUT.kicad_pcb REF --pitch 5.08 --body-dia 14.5 [--drill 1.0] [--pad 2.0] [--unlock]

IN is never modified; OUT must be a different path. The footprint named REF keeps its uuid, position, layer,
Reference/Value/other properties (moved clear of the new body), path/sheetfile and the nets of pads "1" and "2".
Its old graphics and pads are replaced by: two round plated through-hole pads `pitch` apart on the x axis, a
silkscreen circle and a courtyard circle around the body. `--unlock` drops `(locked yes)` so the placer may
move it. The drill and pad sizes are NOT taken from a datasheet: check the part's lead diameter and set
--drill (lead + about 0.2 mm) before ordering. Text-level edit; every other byte of the board is untouched.
"""

import re
import sys


def block_at(text, start):
    depth, i, in_str = 0, start, False
    while i < len(text):
        c = text[i]
        if in_str:
            if c == "\\":
                i += 1
            elif c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return start, i + 1
        i += 1
    raise ValueError("unbalanced")


def child_blocks(block):
    """Top-level child blocks of a block text, as (start, end) offsets relative to block."""
    out, i = [], 1
    while i < len(block) - 1:
        if block[i] == "(":
            s, e = block_at(block, i)
            out.append((s, e))
            i = e
        else:
            i += 1
    return out


def main(argv):
    lib = argv[argv.index("--lib") + 1] if "--lib" in argv else None
    if len(argv) < 3 or (lib is None and ("--pitch" not in argv or "--body-dia" not in argv)):
        sys.exit(__doc__)
    src, dst, ref = argv[0], argv[1], argv[2]
    if src == dst:
        sys.exit("refusing to overwrite the input")

    def opt(name, default=None):
        return float(argv[argv.index(name) + 1]) if name in argv else default

    pitch, body = opt("--pitch", 0.0), opt("--body-dia", 0.0)
    drill, pad = opt("--drill", 1.0), opt("--pad", 2.0)
    text = open(src, encoding="utf-8").read()
    m = re.search(r'\(property "Reference" "%s"' % re.escape(ref), text)
    if not m:
        sys.exit(f"no footprint {ref}")
    fstart = text.rfind("\t(footprint ", 0, m.start())
    fstart = text.index("(footprint", fstart)
    s, e = block_at(text, fstart)
    old = text[s:e]
    nets = {}
    for cs, ce in child_blocks(old):
        child = old[cs:ce]
        if child.startswith("(pad "):
            pm = re.match(r'\(pad "([^"]+)"', child)
            nm = re.search(r"\(net [^)]*\)", child)
            if pm and nm:
                nets[pm.group(1)] = nm.group(0)
    if set(nets) != {"1", "2"}:
        sys.exit(f"{ref}: expected pads 1 and 2 with nets, found {sorted(nets)}")
    keep = []
    for cs, ce in child_blocks(old):
        child = old[cs:ce]
        head = child.split()[0].lstrip("(")
        if head in ("fp_line", "fp_circle", "fp_rect", "fp_poly", "fp_arc", "pad", "model", "attr", "embedded_fonts", "locked") and not (
            head == "locked" and "--unlock" not in argv
        ):
            continue
        keep.append(child)
    head_line = old[: old.index("\n")]
    lib_kids = []
    if lib:
        ltext = open(lib, encoding="utf-8").read()
        ls, le = block_at(ltext, ltext.index("(footprint"))
        lblock = ltext[ls:le]
        ys = []
        for cs, ce in child_blocks(lblock):
            child = lblock[cs:ce]
            h = child.split()[0].lstrip("(")
            if h == "pad":
                num = re.match(r'\(pad "([^"]+)"', child).group(1)
                if num not in nets:
                    sys.exit(f"library pad {num} has no net in the old footprint")
                child = child[:-1].rstrip() + "\n\t\t\t" + nets[num] + "\n\t\t)"
            if h in ("fp_line", "fp_circle", "fp_rect", "fp_poly", "fp_arc", "pad", "attr"):
                lib_kids.append(child)
            for y in re.findall(r"\(\w+ [-\d.]+ ([-\d.]+)\)", child):
                ys.append(abs(float(y)))
        body = 2 * max(ys or [3.0]) - 4.4
    dy = body / 2 + 2.2
    out = [head_line]
    for k in keep:
        if k.startswith('(property "Reference"'):
            k = re.sub(r"\(at [-\d.]+ [-\d.]+( [-\d.]+)?\)", f"(at 0 {-dy:g}\\1)", k, count=1)
        elif k.startswith('(property "Value"'):
            k = re.sub(r"\(at [-\d.]+ [-\d.]+( [-\d.]+)?\)", f"(at 0 {dy:g}\\1)", k, count=1)
        out.append("\t\t" + k)
    if "--unlock" not in argv and "(locked yes)" in old:
        out.insert(1, "\t\t(locked yes)")
    if lib:
        out.extend("\t\t" + k for k in lib_kids)
    else:
        out.append("\t\t(attr through_hole)")
        out.append(f'\t\t(fp_circle (center 0 0) (end {body / 2 + 0.1:g} 0) (stroke (width 0.12) (type solid)) (fill no) (layer "F.SilkS"))')
        out.append(f'\t\t(fp_circle (center 0 0) (end {body / 2 + 0.5:g} 0) (stroke (width 0.05) (type solid)) (fill no) (layer "F.CrtYd"))')
        for num, x in (("1", -pitch / 2), ("2", pitch / 2)):
            out.append(
                f'\t\t(pad "{num}" thru_hole circle (at {x:g} 0) (size {pad:g} {pad:g}) (drill {drill:g}) '
                f'(layers "*.Cu" "*.Mask") (remove_unused_layers no) {nets[num]})'
            )
    out.append("\t)")
    new = "\n".join(out)
    open(dst, "w", encoding="utf-8", newline="").write(text[:s] + new + text[e:])
    print(f"{ref}: {len(old)} -> {len(new)} bytes; pads 1,2 at +-{pitch / 2:g} mm, drill {drill}, nets {nets}")


if __name__ == "__main__":
    main(sys.argv[1:])
