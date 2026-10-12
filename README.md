# autolayout

Open-source placement and routing for KiCad boards, in Rust. Early: the
lossless file layer works (every board in the test corpus round-trips
byte-exact); placement is next. See [ROADMAP.md](ROADMAP.md).

```sh
cargo build --release
./target/release/autolayout roundtrip board.kicad_pcb
./target/release/autolayout info board.kicad_pcb
```

Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT) at
your option.
