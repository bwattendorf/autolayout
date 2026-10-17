# Connection-grade placement (2026-10-16)

The owner's idea: grade every part by how many other parts it connects to, then put low-grade parts right at the hub they serve and give the hubs the room. Built as `autolayout place ... --grade` (off by default) and `autolayout grades PCB [--top N]`.

## What it does
- **Grade** (`place::grade`): the number of other parts a part shares a signal net with. Nets with more than 16 pads (`PLANE_FANOUT`) are planes or rails and are left out. Nothing looks at a name (rule 2). A link records which pad of this part meets which pad of the other, and the net's pad count (a two-pad net is a direct wire and pulls harder, weight `1 / (pads - 1)`).
- **`--grade` in `place::place`:** legalisation order is MCU first, then descending grade (hubs before the parts that serve them), then pad count and courtyard area as before; a part's search target is the weighted average of the matching pad positions of the parts it links that are already placed (its pad onto theirs), else the spring-model target as before. A recorded decoupling pair (`--pairs`) still takes precedence. The rest (escape halo, courtyard legality, front side) is unchanged.
- On m1 the grades are: U1 (the MCU) 70, relays and converters 14, then a long tail (24 parts at 0, 17 at 1, 38 at 2). Some two-pad parts on shared rails of up to 16 pads (D30, R43, D1) score 11-13 because the plain count rates a rail as many connections; the order uses the plain count as asked, the pull uses link weights.

## Measured: place from the service's freeplace board, route with the same router, KiCad 9.0.1 judges (`tools/m7_place_compare.py --variant grade=--grade --judge`), one board at a time
| board | variant | footprints | courtyard overlaps | KiCad unconnected | track mm | vias |
|---|---|---|---|---|---|---|
| m1 | default | 183 | 2 | 0 | 3018 | 469 |
| m1 | **grade** | 183 | 2 | 0 | **2719** | 444 |
| m1 | service | 183 | 2 | 0 | 3040 | 468 |
| c2g | default | 224 | 1 | 0 | 5844 | 659 |
| c2g | grade | 224 | 1 | 0 | 5992 | 651 |
| g2 | default | 190 | 0 | 0 | 3888 | 619 |
| g2 | grade | 190 | 0 | 0 | 3444 | 607 |
| e2 | default | 230 | 0 | 0 | 5169 | 609 |
| e2 | grade | 230 | 0 | 0 | 5313 | 604 |
| cm | default | 177 | 5 | 0 | 5198 | 474 |
| cm | grade | 177 | 5 | 0 | 5118 | 436 |
- KiCad reports 0 unconnected and no clearance, shorting or hole-to-hole violation for every row (default, grade and service) on all five boards; the service rows are in `out/grade_<sku>`.
- Our own file verifier read g2's grade variant as 1 unconnected (128 of 129 nets); KiCad reads 0. The difference is not examined (the verifier ran on the stripped board, KiCad on the poured and refilled one).
- Track length, grade against default: m1 -10%, c2g +2.5%, g2 -11%, e2 +2.8%, cm -1.5%; vias -5%, -1%, -2%, -1%, -8%. So grade placement is as routable as the default on these five boards and is shorter on three; it is not a clear win on all.
- **Clumping is not measured.** The ring analysis (`autolayout capacity`) finds ten times or more the room each part's nets need around every large part, in the default and the grade placement of m1, so neither is clumped by that measure. The owner remembers parts clumped together on earlier runs; no metric here shows it yet.

## Not done
- Boards where the placer was weak before (c4: 2 parts off the board, 17 courtyard overlaps; m2; c2) were not run with `--grade`.
- Hubs do not get extra room yet (the escape halo is still the old per-part rule with fixed 0.65 mm pitch constants; it could use `capacity::gap_for_nets` and the project rules).
- Default stays off.
