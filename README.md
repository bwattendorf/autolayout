# autolayout

Open-source placement and routing for KiCad boards, written in Rust. Every
number below was measured by KiCad itself (`kicad-cli pcb drc`, pcbnew) on a
corpus of real boards, not by this code grading its own output.

**Status:** research stage. The file layer, board model, grader, placer and our
own router all work and are checked against KiCad. Escaping the MCU pins on the
older candidate placements and plane-island handling are still open. See
[ROADMAP.md](ROADMAP.md) for every round, including the ones that got worse.

## Results so far

| What | Result |
|---|---|
| Lossless KiCad file layer | 116 / 116 corpus `.kicad_pcb` files round-trip byte-exact; 20,000 fuzzed inputs, 0 panics |
| Board model vs KiCad | 523 / 523 pads match pcbnew after move, rotate and flip |
| Grader vs KiCad DRC | courtyard overlaps match KiCad exactly on 35 / 35 placed boards |
| Own router vs Freerouting, on boards placed by a commercial layout service | **0 unconnected vs Freerouting's 12–36**, in seconds instead of 118–2,326 s (m1, emu, g2, t) |
| Own router, whole corpus (37 boards) | 19 boards with 0 KiCad routing violations; total unconnected cut from 307 to 141 over the rounds |
| Own placer, judged by routability | within 0–2 unconnected of the commercial return on 15 / 15 boards, with shorter track on every one and 0 KiCad clearance errors |

## Build

```sh
cargo build --release
./target/release/autolayout roundtrip board.kicad_pcb   # lossless read/write check
./target/release/autolayout info board.kicad_pcb        # footprints, nets, tracks, vias
./target/release/autolayout grade board.kicad_pcb       # overlaps, off-board parts, wirelength
```

Crates: `sexpr` (lossless S-expression layer), `board` (board model),
`grade` (grader), `place` (placer), `route` (router), `cli`.

## How it's built

I write the specs, the measurements and the acceptance gates; most of the Rust
is written with Claude Code under that review. Nothing counts as done until
KiCad agrees, and every claim in the roadmap names the command that measured it.

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
