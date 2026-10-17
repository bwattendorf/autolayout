# route --explain, existing-copper seeds, retry of failed nets (2026-10-16)

- `route --explain` prints, per unreached pad, which end of its net is boxed in (grid cells reached
  from each side) and what blocks the frontier (fixed obstacles / plane claims / other nets).
- Nets with same-net copper already attached to a pad start the search from grid nodes on that copper
  (routed first; plane taps keep 1.2 mm clear of planned exits).
- Negotiation retries failed nets every iteration (fixed m1/candidate4).
- Measured (c4/returned/candidate2, `route --explain`): 174/185 nets, 12 unconnected. Every one of the
  12 unreached pads is boxed in, mostly at U1; blockers are a mix of fixed obstacles and other nets.
  Our placer on the same rules (tools/m7_place_compare.py, KiCad-judged): default 30 unconnected,
  --grade 24, service 12.
- 37-board walk before explain: 126 vs 125 unconnected, +20% time (measured, out/walk1).
