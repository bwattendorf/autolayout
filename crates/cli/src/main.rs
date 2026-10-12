//! `autolayout` command line.
//!
//!   autolayout roundtrip <file.kicad_pcb>...        parse + rewrite in memory, report byte-exactness
//!   autolayout info <file.kicad_pcb> [<file.net>]   board summary; with a netlist, the pintype join

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

use autolayout_board::{Board, Netlist, Side};
use autolayout_sexpr::{Document, parse};

const USAGE: &str =
  "usage:\n  autolayout roundtrip <file.kicad_pcb>...\n  autolayout info <file.kicad_pcb> [<file.net>]";

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let result = match args.split_first() {
    Some((cmd, files)) if cmd == "roundtrip" && !files.is_empty() => roundtrip(files),
    Some((cmd, files)) if cmd == "info" && (1..=2).contains(&files.len()) => match files {
      [pcb] => info(pcb, None),
      [pcb, net] => info(pcb, Some(net)),
      _ => Err(USAGE.to_owned()),
    },
    _ => Err(USAGE.to_owned()),
  };
  match result {
    Ok(true) => ExitCode::SUCCESS,
    Ok(false) => ExitCode::FAILURE,
    Err(msg) => {
      eprintln!("{msg}");
      ExitCode::from(2)
    }
  }
}

/// Returns Ok(true) when every file round-trips byte-exact.
fn roundtrip(files: &[String]) -> Result<bool, String> {
  let mut failed = 0;
  for file in files {
    let verdict = match load(file) {
      Ok((src, doc)) if doc.to_string() == src => "OK   byte-exact".to_owned(),
      Ok(_) => "FAIL output differs".to_owned(),
      Err(e) => format!("FAIL {e}"),
    };
    if verdict.starts_with("FAIL") {
      failed += 1;
    }
    println!("{verdict}  {file}");
  }
  println!(
    "{} of {} round-trip byte-exact",
    files.len() - failed,
    files.len()
  );
  Ok(failed == 0)
}

fn info(file: &str, netlist: Option<&str>) -> Result<bool, String> {
  let (src, doc) = load(file)?;
  let Some(root) = doc.root() else {
    return Err(format!("{file}: no top-level list"));
  };
  println!(
    "{file}: ({}) {} bytes",
    root.head().unwrap_or_default(),
    src.len()
  );
  let mut counts: BTreeMap<String, usize> = BTreeMap::new();
  for child in root.items.iter().filter_map(|n| n.as_list()) {
    *counts
      .entry(child.head().unwrap_or_default().into_owned())
      .or_default() += 1;
  }
  for (keyword, n) in counts {
    println!("  {n:>6}  {keyword}");
  }

  let Ok(board) = Board::read(&doc) else {
    return Ok(true);
  };
  let pads: usize = board.footprints.iter().map(|f| f.pads.len()).sum();
  let back = board.footprints.iter().filter(|f| f.side == Side::Back).count();
  let locked = board.footprints.iter().filter(|f| f.locked).count();
  let no_courtyard = board
    .footprints
    .iter()
    .filter(|f| f.courtyard_local.is_none())
    .count();
  println!("board model:");
  println!(
    "  footprints {}  ({back} on back, {locked} locked, {no_courtyard} without courtyard)",
    board.footprints.len()
  );
  println!("  pads {pads}  nets {}", board.nets.len());
  let mut attrs: BTreeMap<&str, usize> = BTreeMap::new();
  for a in board.footprints.iter().flat_map(|f| &f.attrs) {
    *attrs.entry(a).or_default() += 1;
  }
  println!("  attrs {attrs:?}");
  match board.outline {
    Some(o) => println!("  outline {:.2} x {:.2} mm", o.width(), o.height()),
    None => println!("  outline: none on Edge.Cuts"),
  }

  if let Some(net_file) = netlist {
    let (_, net_doc) = load(net_file)?;
    let nl = Netlist::read(&net_doc).map_err(|e| format!("{net_file}: {e}"))?;
    let mut types: BTreeMap<&str, usize> = BTreeMap::new();
    let mut missing = 0;
    for fp in &board.footprints {
      for pad in fp.pads.iter().filter(|p| p.net.is_some()) {
        match nl.pintype(&fp.reference, &pad.number) {
          Some(t) => *types.entry(t).or_default() += 1,
          None => missing += 1,
        }
      }
    }
    println!("netlist join ({net_file}):");
    println!("  pintypes on netted pads {types:?}");
    println!("  netted pads with no netlist entry: {missing}");
  }
  Ok(true)
}

fn load(file: &str) -> Result<(String, Document), String> {
  let src = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
  let doc = parse(&src).map_err(|e| format!("{file}: {e}"))?;
  Ok((src, doc))
}
