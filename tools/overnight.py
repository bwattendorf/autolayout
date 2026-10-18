"""Overnight sweep: try a ladder of router settings on every board, let KiCad judge each, keep the best.

Run under KiCad's bundled Python (it needs pcbnew), from the repo root, after building the router:

    "C:/Program Files/KiCad/10.0/bin/python.exe" tools/overnight.py OUT_DIR --hours 8 [--boards GLOB ...]
        [--skip SUBSTR,...] [--variants FILE] [--resume]

AUTOLAYOUT_CORPUS, KICAD_CLI and AUTOLAYOUT default to this machine's corpus, KiCad 10 and
target/release/autolayout.exe (build it first: `bash tools/cargo.sh build --release`); set them in the
environment to use others.

What it does (nothing here changes the router or any input board):
  * Pass k runs variant k (a set of extra `autolayout route` flags) on every board that is not yet
    solved, so if the time runs out every board has had the cheap variants before any has had the
    expensive ones. A board is solved when KiCad reports 0 unconnected and no more clearance /
    shorting / hole-to-hole violations than the board had before routing.
  * Each attempt is `tools/m7_batch.py`'s `do_board` (strip, pour planes with pcbnew, route, refill
    zones, `kicad-cli pcb drc`), so the numbers are KiCad's, not ours.
  * After every attempt it appends one line to results.jsonl, rewrites best.csv and
    morning_report.md, and copies the best routed board so far for each board to best/.
  * `--resume` continues in an existing OUT_DIR: attempts already in results.jsonl are skipped.
  * `--hours H` stops starting new attempts after H hours (an attempt in progress finishes).
  * Same input + same variant => same output (the router is deterministic), so a result can be
    reproduced from the variant's flags, which are in the report.

The default variants are in DEFAULT_VARIANTS below; `--variants FILE` takes one flag set per line
(`#` comments allowed; an empty line means the router's defaults).
"""

import csv
import glob
import json
import os
import re
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

# Defaults for this machine, so the command line needs no environment setup. Anything already set wins.
for _name, _path in (
    ("AUTOLAYOUT_CORPUS", "C:/Portcullis/controller/boards"),
    ("KICAD_CLI", "C:/Program Files/KiCad/10.0/bin/kicad-cli.exe"),
    ("AUTOLAYOUT", os.path.join(os.path.dirname(HERE), "target", "release", "autolayout.exe")),
):
    if _name not in os.environ and os.path.exists(_path):
        os.environ[_name] = _path

import m7_batch  # noqa: E402  (needs pcbnew: run under KiCad's Python; reads the variables above)

# Cheap first. "" is today's standard pipeline, so every board has its baseline row.
DEFAULT_VARIANTS = [
    "",
    "--quick --iters 40",
    "--quick --iters 25",
    "--quick --iters 50",
    "--quick --iters 40 --seed 1",
    "--quick --iters 40 --seed 2",
    "--quick --iters 40 --seed 3",
    "--quick --iters 40 --seed 4",
    "--quick --iters 40 --seed 5",
    "--quick --iters 40 --seed 6",
    "--quick --iters 40 --hist 60",
    "--quick --iters 25 --seed 7",
    "--quick --iters 50 --seed 8",
]


def log(out_dir, msg):
    line = time.strftime("%Y-%m-%d %H:%M:%S ") + msg
    print(line, flush=True)
    with open(os.path.join(out_dir, "overnight.log"), "a", encoding="utf-8") as f:
        f.write(line + "\n")


def key_of(row):
    """Lower is better: KiCad unconnected, then routing violations beyond what the board had before."""
    if row.get("status") != "ok":
        return (10**9, 10**9)
    got = row.get("kicad_unconnected_after")
    # 0 is a real count (solved), so test for "missing", not for falsy.
    unconnected = 10**9 if got is None or got == "" else int(got)
    before = sum(int(row.get(k) or 0) for k in ("kicad_clearance_before", "kicad_shorting_items_before", "kicad_hole_to_hole_before"))
    return (unconnected, max(0, m7_batch.violations(row) - before))


def solved(row):
    k = key_of(row)
    return k[0] == 0 and k[1] == 0


def load_results(path):
    rows = []
    if os.path.exists(path):
        for line in open(path, encoding="utf-8"):
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def write_reports(out_dir, boards, results):
    """best.csv and morning_report.md from all attempts so far."""
    best = {}
    for r in results:
        b = r["board"]
        if b not in best or key_of(r) < key_of(best[b]):
            best[b] = r
    tried = {}
    for r in results:
        tried.setdefault(r["board"], []).append(r)
    cols = ["board", "baseline_unconnected", "best_unconnected", "best_violations_over_before", "best_variant", "attempts", "seconds"]
    with open(os.path.join(out_dir, "best.csv"), "w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(cols)
        for b in boards:
            if b not in best:
                continue
            base = next((r for r in tried[b] if r["variant"] == ""), None)
            w.writerow([
                b,
                (base or {}).get("kicad_unconnected_after", ""),
                best[b].get("kicad_unconnected_after", ""),
                key_of(best[b])[1] if best[b].get("status") == "ok" else "",
                best[b]["variant"] or "(defaults)",
                len(tried[b]),
                round(sum(float(x.get("wall_s") or 0) for x in tried[b])),
            ])
    n_ok = sum(1 for b in boards if b in best and solved(best[b]))
    with open(os.path.join(out_dir, "morning_report.md"), "w", encoding="utf-8") as f:
        f.write("# Overnight sweep\n\n")
        f.write(f"Written {time.strftime('%Y-%m-%d %H:%M:%S')}. Numbers are KiCad's (`kicad-cli pcb drc`, zones refilled), "
                "measured by tools/m7_batch.py; nothing here is estimated.\n\n")
        f.write(f"**{n_ok} of {len(boards)} boards solved** (KiCad 0 unconnected and no new clearance/shorting/hole-to-hole "
                f"violations); {len(results)} attempts.\n\n")
        f.write("| board | baseline (defaults) | best | violations over before | best variant | attempts |\n|---|---|---|---|---|---|\n")
        for b in boards:
            if b not in best:
                f.write(f"| {b} | not run | | | | 0 |\n")
                continue
            base = next((r for r in tried[b] if r["variant"] == ""), None)
            k = key_of(best[b])
            f.write(f"| {b} | {(base or {}).get('kicad_unconnected_after', '')} | "
                    f"{best[b].get('kicad_unconnected_after', best[b].get('status'))} | "
                    f"{k[1] if best[b].get('status') == 'ok' else ''} | `{best[b]['variant'] or '(defaults)'}` | {len(tried[b])} |\n")
        failed = [r for r in results if r.get("status") != "ok"]
        if failed:
            f.write("\n## Attempts that failed\n\n")
            for r in failed[-30:]:
                f.write(f"- {r['board']} `{r['variant']}`: {r.get('status')} {r.get('error', '')}\n")
        f.write("\nTo reproduce one result: `autolayout route <prepared board> <plane flags> <variant flags>` "
                "(see each attempt's folder under OUT_DIR for the prepared board and log).\n")


def copy_best(out_dir, results, board, work):
    """Keep the best routed board so far for this board under best/."""
    cands = [r for r in results if r["board"] == board]
    top = min(cands, key=key_of)
    if top.get("work") != work:
        return
    src = os.path.join(work, "b_routed.kicad_pcb")
    if not os.path.exists(src):
        return
    dst_dir = os.path.join(out_dir, "best")
    os.makedirs(dst_dir, exist_ok=True)
    stem = re.sub(r"[^A-Za-z0-9_.-]+", "__", board)
    shutil.copyfile(src, os.path.join(dst_dir, stem + ".kicad_pcb"))
    pro = os.path.join(work, "b_routed.kicad_pro")
    if os.path.exists(pro):
        shutil.copyfile(pro, os.path.join(dst_dir, stem + ".kicad_pro"))


def main(argv):
    if not argv or argv[0].startswith("--"):
        sys.exit(__doc__)
    out_dir, args = argv[0], argv[1:]
    corpus = os.environ.get("AUTOLAYOUT_CORPUS")
    if not corpus:
        sys.exit("AUTOLAYOUT_CORPUS is not set")
    m7_batch.CORPUS = corpus
    hours, globs, skip, variants_file, resume = 8.0, [], [], None, False
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--hours":
            hours = float(args[i + 1]); i += 2
        elif a == "--boards":
            i += 1
            while i < len(args) and not args[i].startswith("--"):
                globs.append(args[i]); i += 1
        elif a == "--skip":
            skip = [s for s in args[i + 1].split(",") if s]; i += 2
        elif a == "--variants":
            variants_file = args[i + 1]; i += 2
        elif a == "--resume":
            resume = True; i += 1
        else:
            sys.exit(f"unknown argument {a}")
    if os.path.exists(out_dir) and not resume:
        sys.exit(f"{out_dir} already exists; use a fresh directory, or --resume to continue it")
    os.makedirs(out_dir, exist_ok=True)
    if variants_file:
        variants = [l.rstrip("\n").split("#", 1)[0].strip() for l in open(variants_file, encoding="utf-8")]
        variants = list(dict.fromkeys(variants)) if variants else [""]
    else:
        variants = DEFAULT_VARIANTS
    if not globs:
        globs = [f"quilter/{d}/**/*.kicad_pcb" for d in ("fab", "returned", "finish", "routed")]
    found = []
    for g in globs:
        found += sorted(glob.glob(os.path.join(corpus, g), recursive=True))
    paths = [b for b in dict.fromkeys(found)
             if not any(s in os.path.relpath(b, corpus).replace("\\", "/") for s in skip)]
    rel = {p: os.path.relpath(p, corpus).replace("\\", "/") for p in paths}
    boards = [rel[p] for p in paths]
    results_path = os.path.join(out_dir, "results.jsonl")
    results = load_results(results_path)
    done = {(r["board"], r["variant"]) for r in results}
    deadline = time.time() + hours * 3600
    log(out_dir, f"start: {len(boards)} board(s), {len(variants)} variant(s), {hours} h budget, resume={resume}, "
                 f"{len(results)} earlier attempt(s)")
    stop = False
    for vi, variant in enumerate(variants):
        for p in paths:
            b = rel[p]
            best_so_far = [r for r in results if r["board"] == b]
            if best_so_far and solved(min(best_so_far, key=key_of)):
                continue
            if (b, variant) in done:
                continue
            if time.time() > deadline:
                log(out_dir, "time budget used up; stopping")
                stop = True
                break
            work = os.path.join(out_dir, "runs", f"{re.sub(r'[^A-Za-z0-9_.-]+', '__', b)}__v{vi}")
            if os.path.exists(work):
                shutil.rmtree(work)
            os.environ["M7_ROUTE_EXTRA"] = variant
            t0 = time.time()
            log(out_dir, f"[variant {vi}: {variant or '(defaults)'}] {b}")
            try:
                row = m7_batch.do_board(p, work, False)
            except Exception as e:  # never let one attempt stop the night
                row = {"board": b, "status": "FAIL:unexpected", "error": str(e)[:300]}
            row = {k: v for k, v in row.items() if v != ""} | {"board": b, "variant": variant, "work": work,
                                                                "wall_s": round(time.time() - t0, 1)}
            results.append(row)
            done.add((b, variant))
            with open(results_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(row) + "\n")
            k = key_of(row)
            log(out_dir, f"    status {row.get('status')}, KiCad unconnected {row.get('kicad_unconnected_before', '?')} -> "
                         f"{row.get('kicad_unconnected_after', '?')}, violations over before {k[1] if k[0] < 10**9 else '?'}, "
                         f"{row['wall_s']} s")
            copy_best(out_dir, results, b, work)
            write_reports(out_dir, boards, results)
            # Keep the disk in check: only the best attempt's folder and failed ones are worth keeping.
            for r in results:
                w = r.get("work")
                if w and w != work and os.path.exists(w):
                    top = min([x for x in results if x["board"] == r["board"]], key=key_of)
                    if top.get("work") != w and r.get("status") == "ok":
                        for name in os.listdir(w):
                            if name.endswith((".kicad_pcb",)):
                                try:
                                    os.remove(os.path.join(w, name))
                                except OSError:
                                    pass
        if stop:
            break
    write_reports(out_dir, boards, results)
    log(out_dir, f"finished: {len(results)} attempt(s); see {out_dir}/morning_report.md")


if __name__ == "__main__":
    main(sys.argv[1:])
