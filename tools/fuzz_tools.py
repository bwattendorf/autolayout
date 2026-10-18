"""Robustness check: mutate a real board and make sure no autolayout command panics, crashes or hangs.

    python tools/fuzz_tools.py BOARD.kicad_pcb PROJECT.kicad_pro OUT_DIR [--cases 40] [--seed 1]
        [--commands route place-less verify] [--timeout 120]

CLAUDE.md rule 5: board files are untrusted input; the tools return an error, never a panic. Each case
copies the board with one random mutation, then runs `autolayout route --quick --iters 5` and
`autolayout verify` on it (and `model`), and fails the case on: a Rust panic message, an exit code other
than 0, 1 or 2 (0 ok, 1 not clean, 2 a clean error message; 101 is a panic, other codes are crashes), or a timeout. Mutations (deterministic from --seed):
a number replaced by an odd value (huge, tiny, negative, nan, inf, empty), a chunk of lines deleted, a line
duplicated, the file truncated, a layer name swapped. Failing inputs are kept in OUT_DIR for reproduction.
Uses $AUTOLAYOUT or target_dev/release/autolayout.exe.
"""

import os
import random
import re
import shutil
import subprocess
import sys

EXE = os.environ.get("AUTOLAYOUT", os.path.join("target_dev", "release", "autolayout.exe"))
ODD = ["1e308", "-1e308", "1e-308", "0", "-0", "-1", "nan", "inf", "-inf", "", "99999999999999999999", "0.0000001"]
NUMBER = re.compile(r"(?<![\w.\"])-?\d+\.?\d*(?![\w\"])")


def mutate(text, rng):
    kind = rng.choice(["number", "number", "number", "chunk", "dup", "truncate", "layer"])
    if kind == "number":
        spots = [m for m in NUMBER.finditer(text)]
        if not spots:
            return text, "number: none"
        m = rng.choice(spots)
        v = rng.choice(ODD)
        return text[:m.start()] + v + text[m.end():], f"number at {m.start()} -> {v!r}"
    lines = text.split("\n")
    if kind == "chunk":
        a = rng.randrange(len(lines))
        n = rng.choice([1, 3, 10, 50])
        return "\n".join(lines[:a] + lines[a + n:]), f"deleted {n} line(s) at {a}"
    if kind == "dup":
        a = rng.randrange(len(lines))
        return "\n".join(lines[:a] + [lines[a]] * 2 + lines[a + 1:]), f"duplicated line {a}"
    if kind == "truncate":
        cut = rng.randrange(len(text))
        return text[:cut], f"truncated at {cut}"
    names = ["F.Cu", "B.Cu", "In1.Cu", "In2.Cu", "Edge.Cuts"]
    a, b = rng.sample(names, 2)
    return text.replace(f'"{a}"', f'"{b}"', rng.choice([1, 3, 1000])), f"layer {a} -> {b}"


def run(cmd, timeout):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, encoding="utf-8", errors="replace")
        return p.returncode, p.stdout + p.stderr
    except subprocess.TimeoutExpired:
        return "timeout", ""


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    board, pro, out_dir = argv[0], argv[1], argv[2]
    cases, seed, timeout = 40, 1, 120
    i = 3
    while i < len(argv):
        if argv[i] == "--cases":
            cases = int(argv[i + 1]); i += 2
        elif argv[i] == "--seed":
            seed = int(argv[i + 1]); i += 2
        elif argv[i] == "--timeout":
            timeout = int(argv[i + 1]); i += 2
        else:
            sys.exit("unknown " + argv[i])
    os.makedirs(out_dir, exist_ok=False)
    text = open(board, encoding="utf-8").read()
    rng = random.Random(seed)
    bad = 0
    for k in range(cases):
        mutated, what = mutate(text, rng)
        p = os.path.join(out_dir, f"case{k}.kicad_pcb")
        open(p, "w", encoding="utf-8").write(mutated)
        shutil.copyfile(pro, os.path.splitext(p)[0] + ".kicad_pro")
        results = []
        for name, cmd in (
            ("route", [EXE, "route", p, "--plane", "In1.Cu=GND", "--plane", "In2.Cu=+3V3", "--quick", "--iters", "5"]),
            ("model", [EXE, "model", p]),
            ("verify", [EXE, "verify", p]),
        ):
            code, output = run(cmd, timeout)
            panicked = isinstance(output, str) and ("panicked" in output or "stack overflow" in output.lower())
            # 0 = fine, 1 = ran and the board is not clean, 2 = a clean error message (bad input).
            ok = code in (0, 1, 2) and not panicked
            results.append((name, code, ok))
            if not ok:
                bad += 1
                print(f"case {k} [{what}] {name}: exit {code}{' PANIC' if panicked else ''}", flush=True)
                open(os.path.join(out_dir, f"case{k}_{name}.log"), "w", encoding="utf-8").write(output[-3000:] if isinstance(output, str) else "")
        if all(r[2] for r in results):
            os.remove(p)
            os.remove(os.path.splitext(p)[0] + ".kicad_pro")
    print(f"{cases} case(s), {bad} failure(s); inputs of failing cases kept in {out_dir}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main(sys.argv[1:])
