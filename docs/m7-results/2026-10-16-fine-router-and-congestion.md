# Fine-lattice router and tile congestion predictor (2026-10-16, m1/candidate4 only)

Both are new and experimental. Neither is part of the default `route`.

## Fine-lattice router: `autolayout route PCB --fine [--sub N] [--fine-iters N] [--via-cost F] [--fine-slack MM] [--fine-seed S]`
`route::fine`. A lattice `sub` times finer than the grid pitch (default 6), anchored at the grid origin; rules enforced by distance (foreign-pad distance maps, stamped zones for track-track `track + clearance`, via-track, via-via and hole-to-hole, every distance rule + `max_error` + slack); nets routed as trees, PathFinder negotiation with a seeded shuffle; a net still overlapping at the end is dropped, never written; plane nets need a legal via per pad (a plane tap that conflicts is dropped on its own). Refuses a board with existing copper or keepout zones. Same input, options and seed give the same copper (tested). `write::add_routes_fine` writes it through the existing writer path.
Measured on `out/exist/b.kicad_pcb` (m1/candidate4, planes GND and +3V3), our file verifier, not KiCad:
| lattice | signal layers | iterations | nets complete | unconnected | clearance / shorting / hole-to-hole | time |
|---|---|---|---|---|---|---|
| pitch / 6 | 2 | 60 | 89 of 99 | 30 | 0 / 0 / 0 | 175 s |
| pitch / 12 | 2 | 60 | 86 of 99 | not read | not read | 682 s |
| pitch / 6, In2.Cu opened as a third signal layer (capacity test only) | 3 | 60 | 92 of 99 | 10 | 0 / 0 / 0 | 144 s |
Findings: the copper it writes is clean in our verifier (KiCad has not judged it); a finer lattice and more iterations do not make it complete the board; a third layer helps but does not finish it. The nets that stay open are the same ones the grid router leaves open (+1V1, N$35, N$21, N$55, N$24, N$60 and U1 pins on the left and right edges). Bugs found and fixed while building it: vias of one net placed on top of each other (own-via hole spacing), vias over a same-net through-hole pad (via keep-out around every pad drill, any net), and dropping a whole plane net for one conflicting tap (now per tap).

## Tile congestion predictor: `autolayout congestion PCB [--layers N] [--max-layers M] [--tile MM] [--plane L=N ...]`
`route::congestion`. Tiles of `--tile` mm (default 1.5); a tile edge's capacity per layer from the free stretches along it (`track + clearance` formula of `capacity.rs`); layers beyond the board's own are empty except for through-hole pads; a tile's via capacity from its via-legal free area over `0.866 x spacing^2`; nets routed as trees with negotiated congestion; plane pads take a via in the best of their 3 x 3 tiles, through-hole plane pads none; every tile that holds a netted pad lets that net out (edge capacity at least 1 on the pad's layers, because the model cannot see inside a tile). Output: overflow per layer count and the worst places.
Measured, m1/candidate4: tile 1.5 mm gives overflow 0 from one layer up, and our own placer's c4 output is the same. **This does not predict the board's real routability**: the grid and fine routers cannot finish candidate4 on two layers. Tiles of 0.9, 0.6 and 0.45 mm give overflow of 50, 195 and 283 on every layer count from 1 to 4, dominated by tiles with no legal via site and by pad-access edges (artefacts of the tile model, not room the board lacks). So: zero overflow is necessary, not sufficient, and the model as it stands is too coarse at 1.5 mm and too artefact-ridden below. Not usable yet to say how many layers a board needs.
Tests: tracks in open runs, layer naming, and a synthetic wall with a 1.0 mm gap (2 tracks per layer) that 6 nets cannot cross on 1 layer but can on 4.

## Open
- The predictor needs pad access modelled (how many of a part's pads can leave on one layer and how many must drop through a via) before it can say how many layers a board needs. The pin-row formula in the notes: a row at pitch `p` can drop at most `p / 0.8` of its pins to another layer at once (0.8 mm = via spacing).
- The fine router could take the grid router's copper as a start and rip up only the nets around an open one.
