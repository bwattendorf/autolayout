import collections, csv, importlib.util, os, re, subprocess, sys

ROOT = "C:/autolayout"
SRC = ROOT + "/out/m7r8_all"
OUT = ROOT + "/out/r9"
os.makedirs(OUT, exist_ok=True)
spec = importlib.util.spec_from_file_location("drc_compare", ROOT + "/tools/drc_compare.py")
dc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dc)

BOARDS = ["returned/c2/3p5mil-candidate2", "returned/c4/3p5mil-candidate3", "returned/c4/candidate2",
          "returned/emu/candidate2", "returned/m1/candidate4", "returned/m1/candidate6",
          "returned/m1/locked-candidate5", "returned/m1/tp18-candidate2",
          "returned/r/3p5mil-candidate2", "finish/g2i"]
rows = {r["board"]: r for r in csv.DictReader(open(SRC + "/results.csv", encoding="utf-8"))}


def full(b):
    for k in rows:
        if k.startswith("quilter/" + b + "/"):
            return k
    raise SystemExit("no board " + b)


def kind_of(item):
    s = item.split(": ", 1)[1] if ": " in item else item
    if s.startswith("Track") or s.startswith("Arc"):
        return "tv"
    if s.startswith("Via"):
        return "tv"
    if "pad" in s.split("[")[0].lower() or s.startswith("Pad"):
        return "pad"
    if s.startswith("Zone"):
        return "zone"
    return "other"


def pad_ref(item):
    m = re.search(r"pad \S+ \[[^\]]*\] of (\S+)", item, re.I)
    return m.group(1) if m else None


def classify(e):
    ks = [kind_of(i) for i in e["items"]]
    if "zone" in ks:
        return "zone involved"
    if "other" in ks or len(ks) != 2:
        return "other"
    if ks == ["tv", "tv"]:
        return "track/via vs track/via"
    if sorted(ks) == ["pad", "tv"]:
        return "track/via vs pad"
    if ks == ["pad", "pad"]:
        return "pad vs pad"
    return "other"


def new_of(before, after, kind):
    seen = {}
    for e in before:
        seen[dc.key(e)] = seen.get(dc.key(e), 0) + 1
    new = []
    for e in after:
        if e["kind"] != kind:
            continue
        if seen.get(dc.key(e), 0) > 0:
            seen[dc.key(e)] -= 1
        else:
            new.append(e)
    return new


totals = collections.Counter()
summary = []
for b in BOARDS:
    fb = full(b)
    d = SRC + "/" + fb.replace("/", "__")
    pre, post = d + "/b_prepared_drc.rpt", d + "/b_routed_drc.rpt"
    r = rows[fb]
    kinds = ["clearance"]
    if int(r["kicad_shorting_items"] or 0):
        kinds.append("shorting_items")
    if int(r["kicad_hole_to_hole"] or 0):
        kinds.append("hole_to_hole")
    text = []
    for k in kinds:
        p = subprocess.run([sys.executable, ROOT + "/tools/drc_compare.py", pre, post, "--kind", k, "--show", "10"],
                           capture_output=True, text=True)
        text.append("$ python tools/drc_compare.py <dir>/b_prepared_drc.rpt <dir>/b_routed_drc.rpt --kind %s --show 10  (exit %d)\n%s%s"
                    % (k, p.returncode, p.stdout, p.stderr))
    before, after = dc.entries(pre), dc.entries(post)
    new = new_of(before, after, "clearance")
    cls = collections.Counter(classify(e) for e in new)
    fp = collections.Counter()
    for e in new:
        for i in e["items"]:
            if kind_of(i) == "pad":
                fp[pad_ref(i)] += 1
    totals.update(cls)
    sizes = (os.path.getsize(pre), os.path.getsize(post))
    summary.append((b, len([e for e in before if e["kind"] == "clearance"]),
                    len([e for e in after if e["kind"] == "clearance"]), len(new), dict(cls), fp.most_common(6), sizes))
    open(OUT + "/" + fb.replace("/", "__") + ".txt", "w", encoding="utf-8").write("\n".join(text))

lines = []
alltot = collections.Counter()
for b in BOARDS:
    fb = full(b)
    d = SRC + "/" + fb.replace("/", "__")
    after = [e for e in dc.entries(d + "/b_routed_drc.rpt") if e["kind"] == "clearance"]
    c = collections.Counter(classify(e) for e in after)
    fp = collections.Counter()
    for e in after:
        for i in e["items"]:
            if kind_of(i) == "pad":
                fp[pad_ref(i)] += 1
    alltot.update(c)
    lines.append("ALL-AFTER %s | %d entries | %s | pad footprints %s" % (b, len(after), dict(c), fp.most_common(6)))
lines.append("ALL-AFTER TOTAL: " + str(dict(alltot)))
for s in summary:
    lines.append("%s | clr before %d after %d new %d | %s | pad footprints %s | sizes %s" % s)
lines.append("TOTAL new clearance classification: " + str(dict(totals)))
open(OUT + "/summary.txt", "w", encoding="utf-8").write("\n".join(lines))
print("\n".join(lines))
