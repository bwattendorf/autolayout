//! Grouping against answer keys from real boards.
//!
//! The grouping code decides from pintypes, part values and connectivity
//! only. These tests hold it to results established independently: the m1
//! converter membership recorded by Portcullis's place_floorplan step 1
//! (2026-10-12), and part inventories read off the boards (emu has exactly
//! one inductor; its other regulator is a linear LDO). References appear here
//! as *expected answers*, never as inputs to a decision.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::Path;

use autolayout_board::{Board, Netlist};
use autolayout_place::group::{Converter, Grouping, group};
use autolayout_sexpr::parse;

fn load(root: &str, pcb: &str, net: &str) -> (Board, Grouping) {
  let read = |p: &str| parse(&fs::read_to_string(Path::new(root).join(p)).unwrap()).unwrap();
  let board = Board::read(&read(pcb)).unwrap();
  let netlist = Netlist::read(&read(net)).unwrap();
  let g = group(&board, &netlist);
  (board, g)
}

fn refs(board: &Board, v: &[usize]) -> Vec<String> {
  let mut r: Vec<String> = v.iter().map(|&f| board.footprints[f].reference.clone()).collect();
  r.sort();
  r
}

fn by_regulator<'a>(board: &Board, g: &'a Grouping, reg: &str) -> &'a Converter {
  g.converters
    .iter()
    .find(|c| board.footprints[c.regulator].reference == reg)
    .unwrap_or_else(|| panic!("no converter with regulator {reg}"))
}

#[test]
fn m1_converters_match_the_answer_key() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let (board, g) = load(
    &root,
    "quilter/freeplace/m1/m1.kicad_pcb",
    "m1-1door/hardware/kicad/controller.net",
  );
  assert_eq!(g.unjoined_pads, 0);
  assert_eq!(refs(&board, &g.mcus), ["U1"]);
  assert_eq!(g.converters.len(), 3);

  let core = by_regulator(&board, &g, "U1");
  assert!(core.on_mcu);
  assert_eq!(refs(&board, &[core.inductor]), ["L1"]);
  assert_eq!(refs(&board, &core.output_caps), ["C16", "C17"]);
  assert!(
    core.input_caps.is_none(),
    "MCU core input caps must be reported unknown, not guessed"
  );

  // U3 and U4 are the same regulator declared twice with different pin types
  // (U3's SW POWER-OUT, U4's PASSIVE); both must still be found.
  for (reg, l, boot, fb) in [
    ("U3", "L2", "C24", ["R3", "R4"]),
    ("U4", "L3", "C27", ["R5", "R6"]),
  ] {
    let c = by_regulator(&board, &g, reg);
    assert_eq!(refs(&board, &[c.inductor]), [l], "{reg}");
    assert_eq!(refs(&board, &c.boot), [boot], "{reg}");
    assert_eq!(refs(&board, &c.feedback), fb, "{reg}");
  }
}

#[test]
fn m1_every_part_has_one_role_and_the_crystal_is_found() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let (board, g) = load(
    &root,
    "quilter/freeplace/m1/m1.kicad_pcb",
    "m1-1door/hardware/kicad/controller.net",
  );
  assert_eq!(g.role.len(), board.footprints.len(), "one role per footprint");
  // The crystal by circuit shape: Y1 between two nets, each with a 15 pF
  // load cap to ground, reaching the MCU.
  assert_eq!(g.crystals.len(), 1);
  let x = &g.crystals[0];
  assert_eq!(refs(&board, &[x.part]), ["Y1"]);
  assert_eq!(refs(&board, &x.load_caps), ["C18", "C19"]);
  assert_eq!(refs(&board, &x.host.into_iter().collect::<Vec<_>>()), ["U1"]);
  // Locked parts are the designer's: every one keeps the Fixed role.
  for &f in &g.fixed {
    assert_eq!(g.role[f], autolayout_place::group::Role::Fixed);
  }
}

#[test]
fn emu_has_one_switching_converter() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  // The emulator lives beside the controller boards: corpus root is
  // controller/boards, so its netlist is two levels up.
  let (board, g) = load(
    &root,
    "quilter/freeplace/emu/emu.kicad_pcb",
    "../../emulator/hardware/kicad/accessemu.net",
  );
  assert_eq!(
    g.converters.len(),
    1,
    "one inductor on the board; the other regulator is linear"
  );
  assert!(g.converters[0].on_mcu);
  assert_eq!(refs(&board, &g.mcus), ["U1"]);
}

#[test]
fn fixed_output_buck_has_no_divider() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let (board, g) = load(
    &root,
    "quilter/freeplace/r/r.kicad_pcb",
    "r-reader/hardware/kicad/r.net",
  );
  assert_eq!(g.converters.len(), 3);
  let five = by_regulator(&board, &g, "U4");
  assert!(
    five.feedback.is_empty(),
    "a fixed-output part senses its output directly"
  );
  assert_eq!(refs(&board, &five.boot), ["C25"]);
}
