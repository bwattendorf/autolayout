"""Compare footprint REFERENCE sets: netlist vs freeplace board vs returned board.
Reads files only; writes nothing. Run under KiCad's python (needs pcbnew).

    python tools/footprint_diff.py NETLIST.net FREEPLACE.kicad_pcb RETURNED.kicad_pcb

Prints, with value and footprint for each reference:
  dropped   in netlist and freeplace, not in returned
  extra     in returned, not in netlist
  fp_only   in freeplace, not in netlist
  net_only  in netlist, not in freeplace
and the file mtimes (returned older than freeplace => stale return).
Netlists from SKiDL carry no dnp/exclude flag, so those cannot be read here.
"""
import os, re, sys, time
import pcbnew

def net_refs(p):
    t = open(p, encoding="utf-8", errors="ignore").read()
    out = {}
    for m in re.finditer(r'\(comp\s*\(ref "?([^")\s]+)"?\)\s*\(value "([^"]*)"\)\s*\(description "[^"]*"\)\s*\(footprint "([^"]*)"\)', t):
        out[m.group(1)] = (m.group(2), m.group(3))
    return out

def pcb_refs(p):
    b = pcbnew.LoadBoard(p)
    return {f.GetReference(): (f.GetValue(), str(f.GetFPID().GetUniStringLibId())) for f in b.GetFootprints()}

def main(a):
    net, fp, rt = net_refs(a[0]), pcb_refs(a[1]), pcb_refs(a[2])
    print("counts netlist/freeplace/returned:", len(net), len(fp), len(rt))
    for lab, s in (("dropped", [r for r in net if r in fp and r not in rt]),
                   ("extra", [r for r in rt if r not in net]),
                   ("fp_only", [r for r in fp if r not in net]),
                   ("net_only", [r for r in net if r not in fp])):
        print(f"{lab} ({len(s)}):")
        for r in sorted(s):
            v = net.get(r) or fp.get(r) or rt.get(r)
            print(f"  {r}\t{v[0]}\t{v[1]}")
    mt = [os.path.getmtime(x) for x in a[1:3]]
    print("mtime freeplace", time.ctime(mt[0]), "| returned", time.ctime(mt[1]),
          "| returned_older_than_freeplace", mt[1] < mt[0])

if __name__ == "__main__":
    main(sys.argv[1:])
