//! `autolayout` command line.
//!
//!   autolayout roundtrip <file.kicad_pcb>...        parse + rewrite in memory, report byte-exactness
//!   autolayout info <file.kicad_pcb> [<file.net>]   board summary; with a netlist, the pintype join
//!   autolayout move <in> <out> (<REF> <x> <y> <deg>)...   set footprint poses, write a NEW file
//!   autolayout pads <file.kicad_pcb>                 every pad's board position, as autolayout computes it
//!   autolayout grade <file.kicad_pcb>...             placement metrics (overlaps, off-board, wirelength)
//!   autolayout groups <file.kicad_pcb> <file.net>    supplies, MCUs, converters (from structure only)
//!   autolayout parity <file.kicad_pcb> <file.net>    parts, wiring and net names: board vs netlist
//!   autolayout place <in.kicad_pcb> <file.net> <out.kicad_pcb> [--no-halo]   place unlocked parts, write a NEW file, grade it
//!   autolayout strip <in.kicad_pcb> <out.kicad_pcb>  remove tracks, arcs, vias and zones (NEW file)
//!   autolayout flip <in> <out> (<REF>... | ALL)      move footprints to the other side (NEW file)
//!   autolayout unlock <in> <out> <REF>...            unlock footprints for an experiment (NEW file)

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use autolayout_board::{Board, Netlist, Point, Side, set_pose};
use autolayout_sexpr::{Document, parse};

const USAGE: &str = "usage:\n  autolayout roundtrip <file.kicad_pcb>...\n  autolayout info <file.kicad_pcb> [<file.net>]\n  autolayout move <in.kicad_pcb> <out.kicad_pcb> (<REF> <x> <y> <deg>)...\n  autolayout pads <file.kicad_pcb>\n  autolayout grade <file.kicad_pcb>...\n  autolayout groups <file.kicad_pcb> <file.net>\n  autolayout parity <file.kicad_pcb> <file.net>\n  autolayout place <in.kicad_pcb> <file.net> <out.kicad_pcb> [--no-halo]\n  autolayout strip <in.kicad_pcb> <out.kicad_pcb>";

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

/// Supplies, MCUs and converters found from pintypes, part kinds and
/// connectivity.
fn groups(pcb: &str, net: &str) -> Result<bool, String> {
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let (_, net_doc) = load(net)?;
  let netlist = Netlist::read(&net_doc).map_err(|e| format!("{net}: {e}"))?;
  let g = autolayout_place::group::group(&board, &netlist);
  print!("{}", g.report(&board));
  Ok(true)
}

/// Does the board still describe the netlist's circuit? Exit 1 on a parts or
/// wiring mismatch; a rename alone passes.
fn parity(pcb: &str, net: &str) -> Result<bool, String> {
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let (_, net_doc) = load(net)?;
  let netlist = Netlist::read(&net_doc).map_err(|e| format!("{net}: {e}"))?;
  if netlist.net_of.is_empty() {
    return Err(format!("{net}: no nets read; refusing to report a vacuous match"));
  }
  let p = autolayout_board::parity::parity(&board, &netlist);
  println!("{p}");
  Ok(p.is_clean_for_fab())
}

/// Place every unlocked part and write the result to a NEW file, then grade
/// the written file read back off disk (rule 3: not the placer's own state).
/// Read a decoupling-pair record: `{"pairs": [{"cap": "C4", "part": "U1",
/// "pin": "1"}, …]}` — the shape Portcullis's `decouple()` writes. Every
/// reference must resolve on this board; an unknown cap, part or pin is an
/// error, never a silent skip (a record that matches nothing would place as
/// if it did not exist, which is the failure this input exists to end).
fn read_pairs(path: &str, board: &Board) -> Result<Vec<autolayout_place::place::Pair>, String> {
  let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
  let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
  let rows = json
    .get("pairs")
    .and_then(|p| p.as_array())
    .ok_or_else(|| format!("{path}: no \"pairs\" array"))?;
  let index = |r: &str| board.footprints.iter().position(|f| f.reference == r);
  let mut out = Vec::new();
  for row in rows {
    let field = |k: &str| row.get(k).and_then(|v| v.as_str());
    // Rail-only rows (no part/pin) carry no pairing; they are not errors.
    let (Some(cap), Some(part), Some(pin)) = (field("cap"), field("part"), field("pin")) else {
      continue;
    };
    let c = index(cap).ok_or_else(|| format!("{path}: cap {cap} not on the board"))?;
    let h = index(part).ok_or_else(|| format!("{path}: part {part} not on the board"))?;
    let pad = board
      .footprints
      .get(h)
      .and_then(|fp| fp.pads.iter().position(|p| p.number == pin))
      .ok_or_else(|| format!("{path}: {part} has no pad {pin}"))?;
    out.push(autolayout_place::place::Pair { cap: c, host: h, pad });
  }
  if out.is_empty() {
    return Err(format!(
      "{path}: no cap/part/pin pairs read; refusing a vacuous record"
    ));
  }
  Ok(out)
}

fn place(pcb: &str, net: &str, out: &str, escape_halo: bool, pairs: Option<&String>) -> Result<bool, String> {
  if Path::new(out).exists() {
    return Err(format!("{out}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let (_, net_doc) = load(net)?;
  let netlist = Netlist::read(&net_doc).map_err(|e| format!("{net}: {e}"))?;
  let g = autolayout_place::group::group(&board, &netlist);
  let start = std::time::Instant::now();
  // --no-halo reproduces M4.1 exactly. Spreading stays off: it measured
  // worse (see place::Options).
  // Front-only is the default policy; --no-halo reproduces M4.1 exactly, so
  // it also keeps parts on the side they started on.
  let opts = autolayout_place::place::Options {
    escape_halo,
    spread: false,
    front_only: escape_halo,
  };
  let pairs = match pairs {
    Some(path) => read_pairs(path, &board)?,
    None => Vec::new(),
  };
  if !pairs.is_empty() {
    println!("decoupling record: {} cap/pin pair(s)", pairs.len());
  }
  let p = autolayout_place::place::place(&board, &g, opts, &pairs);
  let secs = start.elapsed().as_secs_f64();
  for &f in &p.flips {
    let fp = board.footprints.get(f).ok_or("bad footprint index")?;
    autolayout_board::flip(&mut doc, fp.node_index).map_err(|e| format!("{}: {e}", fp.reference))?;
  }
  for &(f, at) in &p.moves {
    let fp = board.footprints.get(f).ok_or("bad footprint index")?;
    // A flipped part's angle is now 180 − θ; keep it, only move it.
    let rotation = if p.flips.contains(&f) {
      180.0 - fp.rotation
    } else {
      fp.rotation
    };
    set_pose(&mut doc, fp.node_index, at, rotation).map_err(|e| format!("{}: {e}", fp.reference))?;
  }
  if !p.flips.is_empty() {
    let names: Vec<&str> = p
      .flips
      .iter()
      .filter_map(|&f| board.footprints.get(f).map(|fp| fp.reference.as_str()))
      .collect();
    println!("flipped to the front: {}", names.join(" "));
  }
  fs::write(out, doc.to_string()).map_err(|e| format!("{out}: {e}"))?;
  println!(
    "placed {} part(s) in {secs:.2} s; unplaced: {}",
    p.moves.len(),
    p.unplaced.len()
  );
  for &f in &p.unplaced {
    println!(
      "  UNPLACED {}",
      board.footprints.get(f).map_or("?", |fp| fp.reference.as_str())
    );
  }
  let (_, written) = load(out)?;
  let after = Board::read(&written).map_err(|e| format!("{out}: {e}"))?;
  println!("before ({pcb}):\n{}", autolayout_grade::grade(&board));
  println!("after ({out}, read back):\n{}", autolayout_grade::grade(&after));
  Ok(p.unplaced.is_empty())
}

/// Flip the named footprints to the other side (`ALL` flips every one) and
/// write a NEW file.
fn flip_parts(input: &str, output: &str, refs: &[String]) -> Result<bool, String> {
  if Path::new(output).exists() {
    return Err(format!("{output}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(input)?;
  let board = Board::read(&doc).map_err(|e| format!("{input}: {e}"))?;
  let targets: Vec<&autolayout_board::Footprint> = if refs.iter().any(|r| r == "ALL") {
    board.footprints.iter().collect()
  } else {
    refs
      .iter()
      .map(|r| {
        board
          .footprint(r)
          .ok_or_else(|| format!("{input}: no footprint {r}"))
      })
      .collect::<Result<_, _>>()?
  };
  for fp in &targets {
    autolayout_board::flip(&mut doc, fp.node_index).map_err(|e| format!("{}: {e}", fp.reference))?;
  }
  fs::write(output, doc.to_string()).map_err(|e| format!("{output}: {e}"))?;
  println!("wrote {output} ({} footprint(s) flipped)", targets.len());
  Ok(true)
}

/// Unlock the named footprints (remove `(locked yes)` and a bare `locked`
/// atom) and write a NEW file. For experiments that hand a locked part back
/// to the placer; the input is never changed.
fn unlock(input: &str, output: &str, refs: &[String]) -> Result<bool, String> {
  if Path::new(output).exists() {
    return Err(format!("{output}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(input)?;
  let board = Board::read(&doc).map_err(|e| format!("{input}: {e}"))?;
  let mut indices = Vec::new();
  for r in refs {
    let fp = board
      .footprint(r)
      .ok_or_else(|| format!("{input}: no footprint {r}"))?;
    indices.push(fp.node_index);
  }
  let root = doc.root_mut().ok_or_else(|| format!("{input}: no root list"))?;
  for i in indices {
    if let Some(fp) = root.items.get_mut(i).and_then(|n| n.as_list_mut()) {
      fp.items.retain(|n| match n {
        autolayout_sexpr::Node::List(l) => !l.is("locked"),
        autolayout_sexpr::Node::Atom(a) => a.value() != "locked",
      });
    }
  }
  fs::write(output, doc.to_string()).map_err(|e| format!("{output}: {e}"))?;
  println!("wrote {output} ({} footprint(s) unlocked)", refs.len());
  Ok(true)
}

/// Remove all routed copper — top-level tracks, arcs, vias and zones — and
/// write a NEW file. Placement, pads and everything else keep their bytes.
/// Done here rather than through pcbnew, whose 9.0 SWIG binding leaves every
/// later object unusable after `board.Remove()` (tools/route_check.py).
fn strip(pcb: &str, out: &str) -> Result<bool, String> {
  if Path::new(out).exists() {
    return Err(format!("{out}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(pcb)?;
  let root = doc.root_mut().ok_or_else(|| format!("{pcb}: no root list"))?;
  let before = root.items.len();
  let copper = ["segment", "arc", "via", "zone"];
  root
    .items
    .retain(|n| n.as_list().is_none_or(|l| !copper.iter().any(|k| l.is(k))));
  let removed = before - root.items.len();
  fs::write(out, doc.to_string()).map_err(|e| format!("{out}: {e}"))?;
  println!("wrote {out}: removed {removed} copper item(s)");
  Ok(true)
}

/// Which SMD pads have no via site within reach (see grade::escape).
fn escape(files: &[String]) -> Result<bool, String> {
  for file in files {
    let (_, doc) = load(file)?;
    let board = Board::read(&doc).map_err(|e| format!("{file}: {e}"))?;
    let e = autolayout_grade::escape::escape(&board, autolayout_grade::escape::DEFAULT_RULES);
    let pct = if e.smd_pads == 0 {
      0.0
    } else {
      100.0 * (e.smd_pads - e.blocked) as f64 / e.smd_pads as f64
    };
    println!("{file}");
    println!(
      "  SMD pads with a via site: {}/{} ({pct:.1}%)",
      e.smd_pads - e.blocked,
      e.smd_pads
    );
    for (r, b, t) in e.by_part.iter().take(25) {
      println!("    {r:6} {b:3} of {t:3} blocked");
    }
  }
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
      // Angle printed in [0, 360) after rounding, so 359.97 and 0.0 agree
      // with KiCad's own normalisation.
      let angle = ((pad.angle.rem_euclid(360.0) * 10.0).round() / 10.0).rem_euclid(360.0);
      rows.push(format!(
        "{}\t{}\t{}\t{:.4}\t{:.4}\t{:.1}",
        fp.reference,
        pad.number,
        if fp.side == Side::Back { "B" } else { "F" },
        pad.at.x,
        pad.at.y,
        angle
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
    Some((cmd, [pcb, net])) if cmd == "groups" => groups(pcb, net),
    Some((cmd, [pcb, net])) if cmd == "parity" => parity(pcb, net),
    Some((cmd, [pcb, net, out])) if cmd == "place" => place(pcb, net, out, true, None),
    Some((cmd, [pcb, net, out, flag])) if cmd == "place" && flag == "--no-halo" => {
      place(pcb, net, out, false, None)
    }
    Some((cmd, [pcb, net, out, flag, pairs])) if cmd == "place" && flag == "--pairs" => {
      place(pcb, net, out, true, Some(pairs))
    }
    Some((cmd, [pcb, out])) if cmd == "strip" => strip(pcb, out),
    Some((cmd, files)) if cmd == "escape" && !files.is_empty() => escape(files),
    Some((cmd, [input, output, refs @ ..])) if cmd == "flip" && !refs.is_empty() => {
      flip_parts(input, output, refs)
    }
    Some((cmd, [input, output, refs @ ..])) if cmd == "unlock" && !refs.is_empty() => {
      unlock(input, output, refs)
    }
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
