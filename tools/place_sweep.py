"""Placer sweep: our own placement of every freeplace board, then our router, judged by KiCad.

Run under KiCad's bundled Python (it needs pcbnew), from the repo root:

    "C:/Program Files/KiCad/10.0/bin/python.exe" tools/place_sweep.py OUT_DIR --hours 6
        [--boards ID ...] [--variants FILE] [--route-flags "..."] [--resume]

For every `quilter/freeplace/<id>/<id>.kicad_pcb` that has a netlist (`<id>-*/hardware/kicad/*.net`
in the Portcullis tree) and a project file, and for each placement variant (flags of `autolayout
place`; default ladder below):
  1. `autolayout place FREEPLACE NETLIST PLACED [flags]` -> a new board (inputs are never touched);
  2. the project file is copied beside it, and tools/m7_batch.py's `do_board` strips, pours the
     planes with pcbnew, routes (with --route-flags), refills zones and runs `kicad-cli pcb drc`.
The numbers in the report are KiCad's. A board is solved when KiCad reports 0 unconnected and no
more clearance / shorting / hole-to-hole violations than the placed board had before routing.
Courtyard overlaps and parts off board are placement findings: they are counted from the same DRC
run (kinds `courtyards_overlap`, `copper_edge_clearance`) and reported beside the routing numbers.

Outputs in OUT_DIR: results.jsonl (one line per attempt), best.csv, morning_report.md, best/ (the
best routed board per freeplace board), overnight.log. `--resume` skips attempts already recorded.
Same input + same flags => same output (placer and router are deterministic).
"""

import glob
import json
import os
import re
import shutil
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import overnight  # noqa: E402  (sets the default environment, imports m7_batch)
import m7_batch  # noqa: E402

DEFAULT_VARIANTS = ["", "--grade"]


def find_netlist(corpus, board_id):
    for d in sorted(glob.glob(os.path.join(corpus, board_id + "-*"))):
        nets = sorted(glob.glob(os.path.join(d, "hardware", "kicad", "*.net")))
        if len(nets) == 1:
            return nets[0]
    return None


def log(out_dir, msg):
    overnight.log(out_dir, msg)


def report(out_dir, ids, results):
    best = {}
    for r in results:
        b = r["board"]
        if b not in best or overnight.key_of(r) < overnight.key_of(best[b]):
            best[b] = r
    with open(os.path.join(out_dir, "best.csv"), "w", encoding="utf-8") as f:
        f.write("board,variant,kicad_unconnected_after,violations_over_before,courtyards_overlap,nets_complete,nets_total,attempts\n")
        for b in ids:
            if b in best:
                r = best[b]
                f.write(f"{b},{r['variant'] or '(default)'},{r.get('kicad_unconnected_after', '')},"
                        f"{overnight.key_of(r)[1] if r.get('status') == 'ok' else ''},{r.get('courtyards', '')},"
                        f"{r.get('nets_complete', '')},{r.get('nets_total', '')},{sum(1 for x in results if x['board'] == b)}\n")
    n_ok = sum(1 for b in ids if b in best and overnight.solved(best[b]))
    with open(os.path.join(out_dir, "morning_report.md"), "w", encoding="utf-8") as f:
        f.write("# Placer sweep\n\n")
        f.write(f"Written {time.strftime('%Y-%m-%d %H:%M:%S')}. Our placement + our router, judged by KiCad "
                "(`kicad-cli pcb drc`, zones refilled); nothing here is estimated.\n\n")
        f.write(f"**{n_ok} of {len(ids)} freeplace boards placed and routed completely** "
                f"(KiCad 0 unconnected, no new clearance/shorting/hole-to-hole violations); {len(results)} attempts.\n\n")
        f.write("| board | variant | KiCad unconnected | new violations | courtyard overlaps | nets | attempts |\n|---|---|---|---|---|---|---|\n")
        for b in ids:
            if b not in best:
                f.write(f"| {b} | not run | | | | | 0 |\n")
                continue
            r = best[b]
            tried = ", ".join(f"`{x['variant'] or 'default'}`={x.get('kicad_unconnected_after', x.get('status'))}"
                              for x in results if x["board"] == b)
            f.write(f"| {b} | `{r['variant'] or '(default)'}` | {r.get('kicad_unconnected_after', r.get('status'))} | "
                    f"{overnight.key_of(r)[1] if r.get('status') == 'ok' else ''} | {r.get('courtyards', '')} | "
                    f"{r.get('nets_complete', '')}/{r.get('nets_total', '')} | {tried} |\n")
        failed = [r for r in results if r.get("status") != "ok"]
        if failed:
            f.write("\n## Attempts that failed\n\n")
            for r in failed[-30:]:
                f.write(f"- {r['board']} `{r['variant']}`: {r.get('status')} {r.get('error', '')}\n")


def main(argv):
    if not argv or argv[0].startswith("--"):
        sys.exit(__doc__)
    out_dir, args = argv[0], argv[1:]
    corpus = os.environ["AUTOLAYOUT_CORPUS"]
    m7_batch.CORPUS = corpus
    hours, only, variants_file, route_flags, resume = 6.0, [], None, "", False
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--hours":
            hours = float(args[i + 1]); i += 2
        elif a == "--boards":
            i += 1
            while i < len(args) and not args[i].startswith("--"):
                only.append(args[i]); i += 1
        elif a == "--variants":
            variants_file = args[i + 1]; i += 2
        elif a == "--route-flags":
            route_flags = args[i + 1]; i += 2
        elif a == "--resume":
            resume = True; i += 1
        else:
            sys.exit(f"unknown argument {a}")
    if os.path.exists(out_dir) and not resume:
        sys.exit(f"{out_dir} already exists; use a fresh directory, or --resume")
    os.makedirs(out_dir, exist_ok=True)
    if variants_file:
        variants = [l.rstrip("\n").split("#", 1)[0].strip() for l in open(variants_file, encoding="utf-8")]
        variants = list(dict.fromkeys(variants)) or [""]
    else:
        variants = DEFAULT_VARIANTS
    ids, srcs = [], {}
    for p in sorted(glob.glob(os.path.join(corpus, "quilter", "freeplace", "*", "*.kicad_pcb"))):
        bid = os.path.basename(os.path.dirname(p))
        if only and bid not in only:
            continue
        if not os.path.exists(os.path.splitext(p)[0] + ".kicad_pro") or not find_netlist(corpus, bid):
            continue
        ids.append(bid)
        srcs[bid] = p
    results_path = os.path.join(out_dir, "results.jsonl")
    results = overnight.load_results(results_path)
    done = {(r["board"], r["variant"]) for r in results}
    deadline = time.time() + hours * 3600
    log(out_dir, f"start: {len(ids)} board(s) {ids}, variants {variants}, route flags '{route_flags}', {hours} h, "
                 f"{len(results)} earlier attempt(s)")
    stop = False
    for vi, variant in enumerate(variants):
        for bid in ids:
            mine = [r for r in results if r["board"] == bid]
            if mine and overnight.solved(min(mine, key=overnight.key_of)):
                continue
            if (bid, variant) in done:
                continue
            if time.time() > deadline:
                log(out_dir, "time budget used up; stopping")
                stop = True
                break
            work = os.path.join(out_dir, "runs", f"{bid}__p{vi}")
            if os.path.exists(work):
                shutil.rmtree(work)
            os.makedirs(os.path.join(work, "placed"))
            placed = os.path.join(work, "placed", bid + ".kicad_pcb")
            t0 = time.time()
            log(out_dir, f"[placer variant {vi}: {variant or '(default)'}] {bid}")
            row = {"board": bid, "variant": variant, "work": os.path.join(work, "route")}
            try:
                net = find_netlist(corpus, bid)
                code, out, _ = m7_batch.run([m7_batch.AUTOLAYOUT, "place", srcs[bid], net, placed, *variant.split()], timeout=3600)
                open(os.path.join(work, "place.log"), "w", encoding="utf-8").write(out)
                if code != 0 or not os.path.exists(placed):
                    row.update(status="FAIL:place", error=out.strip()[-300:])
                else:
                    shutil.copyfile(os.path.splitext(srcs[bid])[0] + ".kicad_pro", os.path.splitext(placed)[0] + ".kicad_pro")
                    os.environ["M7_ROUTE_EXTRA"] = route_flags
                    res = m7_batch.do_board(placed, os.path.join(work, "route"), False)
                    row.update({k: v for k, v in res.items() if v != ""})
                    row["board"], row["variant"], row["work"] = bid, variant, os.path.join(work, "route")
                    kinds = row.get("kicad_other_kinds") or ""
                    m = re.search(r'"courtyards_overlap": (\d+)', kinds)
                    row["courtyards"] = int(m.group(1)) if m else 0
            except Exception as e:  # never let one attempt stop the night
                row.update(status="FAIL:unexpected", error=str(e)[:300])
            row["wall_s"] = round(time.time() - t0, 1)
            results.append(row)
            done.add((bid, variant))
            with open(results_path, "a", encoding="utf-8") as f:
                f.write(json.dumps(row) + "\n")
            k = overnight.key_of(row)
            log(out_dir, f"    status {row.get('status')}, KiCad unconnected {row.get('kicad_unconnected_before', '?')} -> "
                         f"{row.get('kicad_unconnected_after', '?')}, new violations {k[1] if k[0] < 10**9 else '?'}, "
                         f"nets {row.get('nets_complete', '?')}/{row.get('nets_total', '?')}, {row['wall_s']} s")
            top = min([r for r in results if r["board"] == bid], key=overnight.key_of)
            if top is row:
                src = os.path.join(row["work"], "b_routed.kicad_pcb")
                if os.path.exists(src):
                    os.makedirs(os.path.join(out_dir, "best"), exist_ok=True)
                    shutil.copyfile(src, os.path.join(out_dir, "best", bid + ".kicad_pcb"))
                    pro = os.path.join(row["work"], "b_routed.kicad_pro")
                    if os.path.exists(pro):
                        shutil.copyfile(pro, os.path.join(out_dir, "best", bid + ".kicad_pro"))
            report(out_dir, ids, results)
        if stop:
            break
    report(out_dir, ids, results)
    log(out_dir, f"finished: {len(results)} attempt(s); see {out_dir}/morning_report.md")


if __name__ == "__main__":
    main(sys.argv[1:])
