//! `autolayout` command line.
//!
//!   autolayout roundtrip <file.kicad_pcb>...        parse + rewrite in memory, report byte-exactness
//!   autolayout info <file.kicad_pcb> [<file.net>]   board summary; with a netlist, the pintype join
//!   autolayout move <in> <out> (<REF> <x> <y> <deg>)...   set footprint poses, write a NEW file
//!   autolayout pads <file.kicad_pcb>                 every pad's board position, as autolayout computes it
//!   autolayout grade <file.kicad_pcb>...             placement metrics (overlaps, off-board, wirelength)

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use autolayout_board::{Board, Netlist, Point, Side, set_pose};
use autolayout_sexpr::{Document, parse};

const USAGE: &str = "usage:\n  autolayout roundtrip <file.kicad_pcb>...\n  autolayout info <file.kicad_pcb> [<file.net>]\n  autolayout move <in.kicad_pcb> <out.kicad_pcb> (<REF> <x> <y> <deg>)...\n  autolayout pads <file.kicad_pcb>";

/// Set the pose of each named footprint and write the result to `output`,
/// which must not exist yet (CLAUDE.md rule 1: inputs are never overwritten).
fn move_parts(input: &str, output: &str, moves: &[String]) -> Result<bool, String> {
  if Path::new(output).exists() {
    return Err(format!("{output}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(input)?;
  let board = Board::read(&doc).map_err(|e| format!("{input}: {e}"))?;
  for m in moves.chunks(4) {
    let [reference, x, y, deg] = m else { continue };
    let num = |s: &str| s.parse::<f64>().map_err(|_| format!("not a number: {s}"));
    let fp = board
      .footprint(reference)
      .ok_or_else(|| format!("{input}: no footprint {reference}"))?;
    set_pose(&mut doc, fp.node_index, Point::new(num(x)?, num(y)?), num(deg)?)
      .map_err(|e| format!("{reference}: {e}"))?;
  }
  fs::write(output, doc.to_string()).map_err(|e| format!("{output}: {e}"))?;
  println!("wrote {output} ({} footprint(s) set)", moves.len() / 4);
  Ok(true)
}

/// Placement metrics for each file, read back off disk.
fn grade(files: &[String]) -> Result<bool, String> {
  for file in files {
    let (_, doc) = load(file)?;
    let board = Board::read(&doc).map_err(|e| format!("{file}: {e}"))?;
    println!("{file}");
    println!("{}", autolayout_grade::grade(&board));
  }
  Ok(true)
}

/// Tab-separated `REF PAD X Y ANGLE`, sorted, so it can be diffed against the
/// same listing produced by KiCad (`tools/kicad_pads.py`).
fn pads(file: &str) -> Result<bool, String> {
  let (_, doc) = load(file)?;
  let board = Board::read(&doc).map_err(|e| format!("{file}: {e}"))?;
  let mut rows = Vec::new();
  for fp in &board.footprints {
    for pad in &fp.pads {
      rows.push(format!(
        "{}\t{}\t{:.4}\t{:.4}\t{:.1}",
        fp.reference,
        pad.number,
        pad.at.x,
        pad.at.y,
        pad.angle.rem_euclid(360.0)
      ));
    }
  }
  rows.sort();
  for r in rows {
    println!("{r}");
  }
  Ok(true)
}

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  let result = match args.split_first() {
    Some((cmd, files)) if cmd == "roundtrip" && !files.is_empty() => roundtrip(files),
    Some((cmd, files)) if cmd == "info" && (1..=2).contains(&files.len()) => match files {
      [pcb] => info(pcb, None),
      [pcb, net] => info(pcb, Some(net)),
      _ => Err(USAGE.to_owned()),
    },
    Some((cmd, rest)) if cmd == "move" => match rest {
      [input, output, moves @ ..] if !moves.is_empty() && moves.len() % 4 == 0 => {
        move_parts(input, output, moves)
      }
      _ => Err(USAGE.to_owned()),
    },
    Some((cmd, rest)) if cmd == "pads" && rest.len() == 1 => rest.first().map_or(Ok(false), |f| pads(f)),
    Some((cmd, files)) if cmd == "grade" && !files.is_empty() => grade(files),
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
