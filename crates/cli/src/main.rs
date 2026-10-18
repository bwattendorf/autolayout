//! `autolayout` command line.
//!
//!   autolayout roundtrip <file.kicad_pcb>...        parse + rewrite in memory, report byte-exactness
//!   autolayout info <file.kicad_pcb> [<file.net>]   board summary; with a netlist, the pintype join
//!   autolayout move <in> <out> (<REF> <x> <y> <deg>)...   set footprint poses, write a NEW file
//!   autolayout pads <file.kicad_pcb>                 every pad's board position, as autolayout computes it
//!   autolayout grade <file.kicad_pcb>...             placement metrics (overlaps, off-board, wirelength)
//!   autolayout groups <file.kicad_pcb> <file.net>    supplies, MCUs, converters (from structure only)
//!   autolayout parity <file.kicad_pcb> <file.net>    parts, wiring and net names: board vs netlist
//!   autolayout place <in.kicad_pcb> <file.net> <out.kicad_pcb> [--no-halo] [--grade] [--pairs <file>]   place unlocked parts, write a NEW file, grade it
//!   autolayout grades <file.kicad_pcb> [--top N]    each part's connection grade (other parts it shares a signal net with)
//!   autolayout strip <in.kicad_pcb> <out.kicad_pcb>  remove tracks, arcs, vias and zones (NEW file)
//!   autolayout flip <in> <out> (<REF>... | ALL)      move footprints to the other side (NEW file)
//!   autolayout rules <file.kicad_pcb> [--track W --clearance C --via D/DRILL]   design rules from the project beside the board (or explicit)
//!   autolayout model <file.kicad_pcb> [--class NAME] [--plane LAYER=NET]... [explicit rules]   build the routing grid, report obstacles and pad access
//!   autolayout route <file.kicad_pcb> [--out NEW.kicad_pcb] [--greedy] [--quick] [--explain] [--iters N] [model flags]   route every net (PathFinder, or greedy baseline); with --out, write tracks and vias to a NEW file
//!   autolayout verify <file.kicad_pcb> [--plane LAYER=NET]... [--probe X,Y]...   judge existing copper from the file alone (unconnected, dangling, hole-to-hole, clearance); not KiCad
//!   autolayout planes <file.kicad_pcb>   which nets would take the inner layers as planes (by pad count, not by name); prints tab-separated FLAGS
//!   autolayout unlock <in> <out> <REF>...            unlock footprints for an experiment (NEW file)

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use autolayout_board::{Board, Netlist, Point, Side, flip, set_pose};
use autolayout_sexpr::{Document, parse};

const USAGE: &str = "usage:\n  autolayout roundtrip <file.kicad_pcb>...\n  autolayout info <file.kicad_pcb> [<file.net>]\n  autolayout move <in.kicad_pcb> <out.kicad_pcb> (<REF> <x> <y> <deg>)...\n  autolayout pads <file.kicad_pcb>\n  autolayout grade <file.kicad_pcb>...\n  autolayout groups <file.kicad_pcb> <file.net>\n  autolayout parity <file.kicad_pcb> <file.net>\n  autolayout place <in.kicad_pcb> <file.net> <out.kicad_pcb> [--no-halo]\n  autolayout strip <in.kicad_pcb> <out.kicad_pcb>
  autolayout flip <in.kicad_pcb> <out.kicad_pcb> --small-caps [--near \"x,y;x,y\" --count N]";

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

/// Pad extent (mm) up to which a capacitor counts as small. Electrolytic and
/// other bulk parts have pads spanning more than this; decided from the
/// footprint's own pad geometry, not from a name.
const SMALL_PART_MM: f64 = 3.0;

/// Flip every unlocked front-side capacitor (part kind from its value) whose
/// pads span at most `SMALL_PART_MM` to the back, and write the board to
/// `output`, which must not exist yet.
fn flip_small_caps(input: &str, output: &str, near: &[Point], count: Option<usize>) -> Result<bool, String> {
  if Path::new(output).exists() {
    return Err(format!("{output}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(input)?;
  let board = Board::read(&doc).map_err(|e| format!("{input}: {e}"))?;
  let mut picked: Vec<(usize, String, f64)> = Vec::new();
  for fp in &board.footprints {
    if fp.locked
      || fp.side != Side::Front
      || autolayout_place::kind::kind_of(fp) != autolayout_place::kind::Kind::Capacitor
    {
      continue;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in &fp.pads {
      x0 = x0.min(p.at.x - p.size.x / 2.0);
      x1 = x1.max(p.at.x + p.size.x / 2.0);
      y0 = y0.min(p.at.y - p.size.y / 2.0);
      y1 = y1.max(p.at.y + p.size.y / 2.0);
    }
    if x1 >= x0 && y1 >= y0 && (x1 - x0).max(y1 - y0) <= SMALL_PART_MM {
      let d = near
        .iter()
        .map(|q| (fp.at - *q).length())
        .fold(f64::MAX, f64::min);
      picked.push((fp.node_index, fp.reference.clone(), d));
    }
  }
  // Nearest to the given points first (ties by reference), then the first N.
  picked.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.1.cmp(&b.1)));
  if let Some(n) = count {
    picked.truncate(n);
  }
  for (idx, reference, _) in &picked {
    flip(&mut doc, *idx).map_err(|e| format!("{reference}: {e}"))?;
  }
  fs::write(output, doc.to_string()).map_err(|e| format!("{output}: {e}"))?;
  let refs: Vec<&str> = picked.iter().map(|p| p.1.as_str()).collect();
  println!(
    "wrote {output} ({} small capacitor(s) flipped to the back)
FLIPPED	{}",
    picked.len(),
    refs.join(" ")
  );
  Ok(true)
}

/// Options of `autolayout flip ... --small-caps`: `--near "x,y;x,y"` ranks the
/// capacitors by distance to those points and `--count N` keeps the first N.
fn flip_command(input: &str, output: &str, opts: &[String]) -> Result<bool, String> {
  let (mut near, mut count) = (Vec::new(), None);
  let mut it = opts.iter();
  while let Some(o) = it.next() {
    match o.as_str() {
      "--near" => {
        for pair in it
          .next()
          .map_or("", String::as_str)
          .split(';')
          .filter(|s| !s.is_empty())
        {
          let mut xy = pair.split(',').map(|v| v.trim().parse::<f64>());
          match (xy.next(), xy.next()) {
            (Some(Ok(x)), Some(Ok(y))) if x.is_finite() && y.is_finite() => near.push(Point::new(x, y)),
            _ => return Err(format!("--near: not x,y: {pair}")),
          }
        }
      }
      "--count" => {
        count = Some(
          it.next()
            .and_then(|v| v.parse::<usize>().ok())
            .ok_or("--count needs a number")?,
        );
      }
      other => return Err(format!("flip: unknown option {other}")),
    }
  }
  flip_small_caps(input, output, &near, count)
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

/// `--halo-boost FILE`: one `REFERENCE FACTOR` per line (`#` comments allowed), the factor that
/// multiplies that part's escape halo. The file is data written by the feedback loop for this board
/// (`tools/autoplace.py`), not a rule keyed on names in the placer. Unknown references are an error.
fn read_boost(path: &str, board: &Board) -> Result<Vec<f64>, String> {
  let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
  let mut out = vec![1.0; board.footprints.len()];
  for (n, line) in text.lines().enumerate() {
    let line = line.split('#').next().unwrap_or("").trim();
    if line.is_empty() {
      continue;
    }
    let mut it = line.split_whitespace();
    let (Some(reference), Some(factor), None) = (it.next(), it.next(), it.next()) else {
      return Err(format!("{path}:{}: expected `REFERENCE FACTOR`", n + 1));
    };
    let factor: f64 = factor
      .parse()
      .ok()
      .filter(|f: &f64| f.is_finite() && *f >= 0.0)
      .ok_or_else(|| format!("{path}:{}: bad factor `{factor}`", n + 1))?;
    let at = board
      .footprints
      .iter()
      .position(|f| f.reference == reference)
      .ok_or_else(|| format!("{path}:{}: no part `{reference}` on the board", n + 1))?;
    if let Some(slot) = out.get_mut(at) {
      *slot = factor;
    }
  }
  Ok(out)
}

fn place(
  pcb: &str,
  net: &str,
  out: &str,
  opts: autolayout_place::place::Options,
  pairs: Option<&String>,
  boost_file: Option<&String>,
) -> Result<bool, String> {
  if Path::new(out).exists() {
    return Err(format!("{out}: already exists; refusing to overwrite"));
  }
  let (_, mut doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let (_, net_doc) = load(net)?;
  let netlist = Netlist::read(&net_doc).map_err(|e| format!("{net}: {e}"))?;
  // The project's copper-to-edge clearance decides which small locked parts cannot be made where they sit.
  let edge = autolayout_route::rules::DesignRules::load_for_board(Path::new(pcb))
    .map(|r| r.min.copper_edge_clearance)
    .unwrap_or(0.0);
  let g = autolayout_place::group::group_with(&board, &netlist, edge);
  let released = autolayout_place::group::locked_off_board(&board, edge);
  if !released.is_empty() {
    let names: Vec<&str> = released
      .iter()
      .filter_map(|&f| board.footprints.get(f).map(|fp| fp.reference.as_str()))
      .collect();
    println!(
      "locked part(s) with a pad outside the outline, or a small one closer to the edge than {edge} mm, cannot be made where they sit, so they are placed: {}",
      names.join(" ")
    );
  }
  let start = std::time::Instant::now();
  let pairs = match pairs {
    Some(path) => read_pairs(path, &board)?,
    None => Vec::new(),
  };
  if !pairs.is_empty() {
    println!("decoupling record: {} cap/pin pair(s)", pairs.len());
  }
  let boost = match boost_file {
    Some(path) => read_boost(path, &board)?,
    None => Vec::new(),
  };
  let p = autolayout_place::place::place_with_boost(&board, &g, opts, &pairs, &boost);
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

/// Print the design rules a router would use. With no flags they come from
/// the `.kicad_pro` beside the board and a missing project is an error; with
/// all three flags they are explicit. Anything in between is refused.
fn rules(pcb: &str, flags: &[String]) -> Result<bool, String> {
  let r = if flags.is_empty() {
    autolayout_route::rules::DesignRules::load_for_board(Path::new(pcb))
  } else if flags.len() == 6 {
    explicit_rules(flags)
  } else {
    return Err("rules: give all of --track W --clearance C --via D/DRILL, or none".to_owned());
  }
  .map_err(|e| e.to_string())?;
  print!("{}", r.report());
  Ok(true)
}

fn explicit_rules(
  flags: &[String],
) -> Result<autolayout_route::rules::DesignRules, autolayout_route::rules::RulesError> {
  use autolayout_route::rules::{DesignRules, RulesError};
  let get = |name: &str| {
    flags
      .iter()
      .position(|f| f == name)
      .and_then(|i| flags.get(i + 1))
      .ok_or_else(|| RulesError::Field(name.to_owned()))
  };
  let num = |name: &str, s: &str| s.parse::<f64>().map_err(|_| RulesError::Field(name.to_owned()));
  let track = num("--track", get("--track")?)?;
  let clearance = num("--clearance", get("--clearance")?)?;
  let via = get("--via")?;
  let (d, dr) = via
    .split_once('/')
    .ok_or_else(|| RulesError::Field("--via (D/DRILL)".to_owned()))?;
  DesignRules::explicit(track, clearance, num("--via", d)?, num("--via", dr)?)
}

/// `--class NAME`, `--plane LAYER=NET`..., and optional explicit rules, as the
/// routing commands take them.
type ModelArgs = (
  autolayout_route::rules::DesignRules,
  autolayout_route::rules::NetClass,
  Vec<(String, String)>,
);

fn model_args(pcb: &str, args: &[String]) -> Result<ModelArgs, String> {
  use autolayout_route::rules::DesignRules;
  let (mut class, mut planes, mut rule_flags) = (String::from("Default"), Vec::new(), Vec::new());
  let mut it = args.iter();
  while let Some(a) = it.next() {
    match a.as_str() {
      "--class" => class = it.next().ok_or("--class needs a name")?.clone(),
      "--plane" => {
        let v = it.next().ok_or("--plane needs LAYER=NET")?;
        let (l, n) = v.split_once('=').ok_or("--plane needs LAYER=NET")?;
        planes.push((l.to_owned(), n.to_owned()));
      }
      _ => {
        rule_flags.push(a.clone());
        rule_flags.extend(it.next().cloned());
      }
    }
  }
  let rules = if rule_flags.is_empty() {
    DesignRules::load_for_board(Path::new(pcb))
  } else {
    explicit_rules(&rule_flags)
  }
  .map_err(|e| e.to_string())?;
  let profile = rules
    .classes
    .iter()
    .find(|c| c.name == class)
    .ok_or_else(|| format!("no net class `{class}` in the rules"))?
    .clone();
  Ok((rules, profile, planes))
}

/// Greedy sequential route of every net on the model; reports, writes nothing
/// (writing is M7.4). Copper already on the board is kept as obstacles, so
/// route a stripped board (`autolayout strip`).
/// Tap repair looks at plane-tap vias this many pitches from a pad left open.
const TAP_REPAIR_PITCHES: f64 = 6.0;
/// Tap repair runs only when at most this many pads are left open.
const TAP_REPAIR_MAX_OPEN: usize = 12;
/// Most tap repair rounds.
const TAP_REPAIR_ROUNDS: usize = 4;

fn route(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::{
    driver::route_all,
    model::RoutingModel,
    negotiate::{Params, route_negotiated},
    search::Costs,
  };
  let mut greedy = false;
  let mut quick = false;
  let mut repair = None::<f64>;
  let mut seed = 0u64;
  let mut explain = false;
  let mut lanes = false;
  let mut tap_spread = false;
  let mut auto_planes = false;
  let mut fine: Option<autolayout_route::fine::FineParams> = None;
  let mut out_path: Option<String> = None;
  let mut params = Params::default();
  let mut rest = Vec::new();
  let mut it = args.iter();
  while let Some(a) = it.next() {
    match a.as_str() {
      "--greedy" => greedy = true,
      "--quick" => quick = true,
      "--repair" => {
        repair = Some(
          it.next()
            .and_then(|v| v.parse().ok())
            .ok_or("--repair needs a margin in mm")?,
        );
      }
      "--seed" => {
        seed = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--seed needs a number")?
      }
      "--explain" => explain = true,
      "--lanes" => lanes = true,
      "--tap-spread" => tap_spread = true,
      "--auto-planes" => auto_planes = true,
      "--fine" => fine = Some(autolayout_route::fine::FineParams::default()),
      "--sub" | "--fine-iters" | "--via-cost" | "--fine-slack" | "--fine-seed" => {
        let v = it.next().ok_or_else(|| format!("{a} needs a value"))?;
        let f = fine.get_or_insert_with(autolayout_route::fine::FineParams::default);
        let bad = || format!("{a}: `{v}` is not a number");
        match a.as_str() {
          "--sub" => f.sub = v.parse().map_err(|_| bad())?,
          "--fine-iters" => f.iters = v.parse().map_err(|_| bad())?,
          "--via-cost" => f.via_cost = v.parse().map_err(|_| bad())?,
          "--fine-slack" => f.slack = v.parse().map_err(|_| bad())?,
          _ => f.seed = v.parse().map_err(|_| bad())?,
        }
      }
      "--no-ripup" => params.ripup_retry = false,
      "--out" => out_path = Some(it.next().ok_or("--out needs a path")?.clone()),
      "--iters" => {
        params.max_iterations = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--iters needs a number")?;
      }
      "--hist" => {
        params.history_step = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--hist needs a number")?;
      }
      "--present" => {
        params.present_first = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--present needs a number")?;
      }
      _ => rest.push(a.clone()),
    }
  }
  let (rules, profile, planes) = model_args(pcb, &rest)?;
  if let Some(o) = &out_path {
    let same = fs::canonicalize(o)
      .ok()
      .zip(fs::canonicalize(pcb).ok())
      .is_some_and(|(a, b)| a == b);
    if Path::new(o).exists() || same || o == pcb {
      return Err(format!(
        "{o}: already exists or is the input; refusing to overwrite"
      ));
    }
  }
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  // --auto-planes: with no --plane given, the inner layers become planes by `planes::plan` (the nets
  // with the most pads, never a name), as `autolayout planes` would print them.
  let mut planes = planes;
  if auto_planes && planes.is_empty() {
    let plan = autolayout_route::planes::plan(&doc, &board);
    planes = plan
      .assignments
      .iter()
      .map(|(l, n, _)| (l.clone(), n.clone()))
      .collect();
    let shown: Vec<String> = plan
      .assignments
      .iter()
      .map(|(l, n, k)| format!("{l}={n} ({k} pads)"))
      .collect();
    println!(
      "planes (automatic): {}",
      if shown.is_empty() {
        "none".to_owned()
      } else {
        shown.join(", ")
      }
    );
  }
  let mut m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).map_err(|e| e.to_string())?;
  if lanes {
    println!("lanes planned for {} pads", m.lanes.len());
    if std::env::var("AUTOLAYOUT_LANES").is_ok() {
      let mut v: Vec<_> = m.lanes.iter().collect();
      v.sort_by_key(|(k, _)| **k);
      for (&(f, p), l) in v {
        if let Some(pad) = board.footprints.get(f).and_then(|fp| fp.pads.get(p)) {
          println!(
            "lane {}.{} at ({:.3}, {:.3}) -> {} {} (line at {:.3})",
            board.footprints.get(f).map_or("?", |fp| fp.reference.as_str()),
            pad.number,
            pad.at.x,
            pad.at.y,
            if l.column { "column" } else { "row" },
            l.index,
            if l.column { m.origin.x } else { m.origin.y } + l.index as f64 * m.pitch
          );
        }
      }
    }
  }
  if let Some(fp) = fine {
    return route_fine_cmd(pcb, &doc, &board, &rules, &m, out_path.as_deref(), fp);
  }
  let t = std::time::Instant::now();
  let plane_codes: Vec<(String, u32)> = m
    .planes
    .iter()
    .map(|p| (p.layer.clone(), p.net).clone())
    .collect();
  // One routing attempt at a stub reach, judged from the text it would be
  // written as (the same judge the final report uses).
  let attempt = |m: &RoutingModel, reach: f64, share_stubs: bool| -> Result<_, String> {
    let costs = Costs {
      reach,
      share_stubs,
      explain: explain || repair.is_some(),
      lanes,
      seed,
      tap_spread,
      ..Costs::default()
    };
    let (s, stats) = if greedy {
      (route_all(&board, m, costs), None)
    } else {
      let (s, st) = route_negotiated(&board, m, costs, params);
      (s, Some(st))
    };
    let mut judged = doc.clone();
    let written = autolayout_route::write::add_routes(&mut judged, m, &board, &s)?;
    let jtext = judged.to_string();
    let jdoc = parse(&jtext).map_err(|e| format!("internal: {e}"))?;
    let jboard = Board::read(&jdoc).map_err(|e| e.to_string())?;
    let v = autolayout_route::verify::verify_fresh_planes(&jdoc, &jboard, &rules, &plane_codes);
    Ok((s, stats, judged, written, v, (reach, share_stubs)))
  };
  // The stub settings are a portfolio: stub reach (narrow leaves corridors
  // free, wide reaches pads narrow cannot) and whether stubs of different nets
  // may share corridor cells. None dominates across boards (see
  // `search::NEAR_REACH_NARROW`, `Costs::share_stubs`). Attempts run in this
  // order until one is complete and clean; the one with the fewest
  // verifier-unconnected wins, ties to the earlier.
  use autolayout_route::search::{NEAR_REACH_NARROW, NEAR_REACH_PITCHES};
  type Attempt = (
    autolayout_route::driver::Summary,
    Option<autolayout_route::negotiate::NegotiationStats>,
    autolayout_sexpr::Document,
    autolayout_route::write::Written,
    autolayout_route::verify::VerifyReport,
    (f64, bool),
  );
  let mut best: Attempt = attempt(
    &m,
    if quick {
      NEAR_REACH_PITCHES
    } else {
      NEAR_REACH_NARROW
    },
    true,
  )?;
  // One line per attempt for the report: what it reached, and whether it ran.
  let mut trail: Vec<String> = Vec::new();
  let describe = |a: &Attempt| {
    format!(
      "stub reach {} pitches, shared corridors {}: nets complete {} of {}, verifier unconnected {}, clearance+shorting+hole-to-hole {}, {}",
      a.5.0,
      a.5.1,
      a.0.nets_complete,
      a.0.nets_total,
      a.4.unconnected,
      a.4.clearance + a.4.shorting + a.4.hole_to_hole,
      if a.0.nets_complete == a.0.nets_total && a.4.clean() {
        "clean"
      } else {
        "NOT clean"
      }
    )
  };
  trail.push(format!("  portfolio attempt 1: {}", describe(&best)));
  if !greedy && !quick && !(best.0.nets_complete == best.0.nets_total && best.4.clean()) {
    // The three further attempts are independent of one another (each reads
    // the same model and board), so they run in parallel threads. Their
    // results are folded in the fixed order below, with the same early stop
    // as the one-after-another loop it replaces, so the chosen attempt does
    // not depend on which thread finishes first. A later attempt that the
    // sequential loop would have skipped is computed and then ignored.
    let settings = [
      (NEAR_REACH_PITCHES, true),
      (NEAR_REACH_NARROW, false),
      (NEAR_REACH_PITCHES, false),
    ];
    let results: Vec<Result<Attempt, String>> = std::thread::scope(|scope| {
      let handles: Vec<_> = settings
        .iter()
        .map(|&(reach, share)| {
          let attempt = &attempt;
          let m = &m;
          scope.spawn(move || attempt(m, reach, share))
        })
        .collect();
      handles
        .into_iter()
        .map(|h| {
          h.join()
            .unwrap_or_else(|_| Err("a routing attempt thread panicked".to_string()))
        })
        .collect()
    });
    for (i, next) in results.into_iter().enumerate() {
      if best.0.nets_complete == best.0.nets_total && best.4.clean() {
        trail.push(format!(
          "  portfolio attempt {}: not needed, an earlier attempt was clean (computed in parallel, result discarded)",
          i + 2
        ));
        continue;
      }
      let next = next?;
      trail.push(format!("  portfolio attempt {}: {}", i + 2, describe(&next)));
      let key = |a: &Attempt| (a.4.unconnected, a.4.clearance + a.4.shorting + a.4.hole_to_hole);
      if key(&next) < key(&best) {
        best = next;
      }
    }
  } else if !greedy && !quick {
    trail.push("  portfolio attempts 2-4: not run, attempt 1 was clean".to_string());
  }
  // Island repair: a plane tap the emulated pour leaves cut off from its
  // plane (`plane_island`) is moved by forbidding its via site and routing
  // again with the same settings. Kept only if it leaves fewer unconnected (or
  // as many with fewer islands); otherwise the sites are unblocked and the
  // earlier result stands. At most three rounds.
  let mut repair_rounds = 0usize;
  if !greedy && !quick {
    for _ in 0..3 {
      let islands: Vec<_> = best
        .4
        .issues
        .iter()
        .filter(|i| i.kind == "plane_island")
        .map(|i| i.at)
        .collect();
      if islands.is_empty() {
        break;
      }
      let saved = m.via.clone();
      if islands.iter().filter(|&&p| m.block_via_near(p)).count() == 0 {
        break;
      }
      let (reach, share) = best.5;
      let next = attempt(&m, reach, share)?;
      let key = |a: &Attempt| {
        (
          a.4.unconnected,
          a.4.plane_islands,
          a.4.clearance + a.4.shorting + a.4.hole_to_hole,
        )
      };
      if key(&next) < key(&best) {
        trail.push(format!(
          "  island repair round {}: kept, {}",
          repair_rounds + 1,
          describe(&next)
        ));
        best = next;
        repair_rounds += 1;
      } else {
        trail.push(format!(
          "  island repair round {}: rejected, {}",
          repair_rounds + 1,
          describe(&next)
        ));
        m.via = saved;
        break;
      }
    }
  }
  // Tap repair: plane taps are placed first and never move, so a tap via can
  // sit in the only exit lane of a fine-pitch pad and leave its net
  // unreachable (fab/emu: U1.79 boxed in by the GND and +3V3 taps of the cap
  // under it, R4.1 left open; see docs/m7-results/2026-10-16-tap-repair.md).
  // For every pad of a net the result left incomplete, each plane-tap via
  // within `TAP_REPAIR_PITCHES` pitches of the pad is forbidden for every net
  // and the board is routed again with the same settings; the replacement
  // taps are then looked at the same way, up to `TAP_REPAIR_ROUNDS` rounds
  // (a moved tap can land in the next lane over). The result replaces the
  // earlier one only if it leaves fewer unconnected (then fewer violations);
  // otherwise every forbidden site is unblocked. Runs only when few pads are open.
  let mut tap_rounds = 0usize;
  if !greedy && !quick {
    let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
    let by_net = board.pads_by_net();
    let tap_sites = |a: &Attempt, m: &RoutingModel| -> Vec<autolayout_board::Point> {
      let mut open: Vec<(usize, usize)> = Vec::new();
      let mut open_pads = 0usize;
      for r in a.0.routes.iter().filter(|r| !r.failed.is_empty()) {
        open_pads += r.failed.len();
        open.extend(by_net.get(&r.net).into_iter().flatten().copied());
      }
      if open.is_empty() || open_pads > TAP_REPAIR_MAX_OPEN {
        return Vec::new();
      }
      let reach = m.pitch * TAP_REPAIR_PITCHES;
      let mut sites = Vec::new();
      for r in a.0.routes.iter().filter(|r| plane_nets.contains(&r.net)) {
        for &(ix, iy) in &r.vias {
          let c = m.centre(ix, iy);
          let near = open.iter().any(|&(f, p)| {
            board
              .footprints
              .get(f)
              .and_then(|fp| fp.pads.get(p))
              .is_some_and(|pad| (pad.at - c).length() <= reach)
          });
          if near {
            sites.push(c);
          }
        }
      }
      sites
    };
    let key = |a: &Attempt| (a.4.unconnected, a.4.clearance + a.4.shorting + a.4.hole_to_hole);
    let saved = m.via.clone();
    let mut sites = tap_sites(&best, &m);
    let mut blocked_total = 0usize;
    let (reach_cfg, share) = best.5;
    for round in 1..=TAP_REPAIR_ROUNDS {
      let newly = sites.iter().filter(|&&c| m.block_via_near(c)).count();
      if newly == 0 {
        break;
      }
      blocked_total += newly;
      let next = attempt(&m, reach_cfg, share)?;
      let after = tap_sites(&next, &m);
      if key(&next) < key(&best) {
        trail.push(format!(
          "  tap repair round {round}: kept ({blocked_total} tap via site(s) forbidden so far), {}",
          describe(&next)
        ));
        best = next;
        tap_rounds = round;
        break;
      }
      trail.push(format!(
        "  tap repair round {round}: not better ({newly} more site(s) forbidden), {}",
        describe(&next)
      ));
      sites = after;
    }
    if tap_rounds == 0 {
      m.via = saved;
    }
  }
  let (s, stats, mut judged, mut written, mut v, chosen) = best;
  // Fine-lattice repair of the pads the grid router left open: for each cluster of them, rip up
  // a window around it, re-route the window with the fine lattice against the copper kept
  // outside it, and keep the result only if the verifier finds fewer unconnected.
  let mut repaired = false;
  if let Some(margin) = repair {
    let st = &m.stats;
    if st.existing_segments + st.existing_vias + st.keepout_zones > 0 {
      println!("repair: skipped, the board has existing copper or keepouts the fine lattice does not read");
    } else {
      use autolayout_route::fine::{FineParams, grid_copper, repair_region};
      let fp = FineParams {
        sub: 12,
        iters: 250,
        slack: -0.003,
        hist_inc: 1.0,
        ..FineParams::default()
      };
      let mut copper = grid_copper(&m, &s);
      // Where each unreached pad is stuck: the region of the side that reached fewer cells.
      let mut open: Vec<autolayout_board::Point> = s
        .routes
        .iter()
        .flat_map(|r| r.why.iter())
        .map(|w| {
          if w.tree_reach <= w.pad_reach {
            w.tree_at
          } else {
            w.pad_at
          }
        })
        .collect();
      open.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
      // Windows: pads closer than the margin share one; the window is their box plus the margin.
      let mut groups: Vec<Vec<autolayout_board::Point>> = Vec::new();
      for p in open {
        match groups
          .iter_mut()
          .find(|g| g.iter().any(|q| (*q - p).length() < 2.0 * margin))
        {
          Some(g) => g.push(p),
          None => groups.push(vec![p]),
        }
      }
      for (gi, g) in groups.iter().enumerate() {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in g {
          x0 = x0.min(p.x);
          y0 = y0.min(p.y);
          x1 = x1.max(p.x);
          y1 = y1.max(p.y);
        }
        // Grow the window until every failed net inside it has something to join to.
        let t = std::time::Instant::now();
        let mut grown = margin;
        let out = loop {
          let lo = |v: f64, o: f64| ((v - grown - o) / m.pitch).floor().max(0.0) as usize;
          let hi = |v: f64, o: f64, n: usize| {
            (((v + grown - o) / m.pitch).ceil().max(0.0) as usize).min(n.saturating_sub(1))
          };
          let window = (
            lo(x0, m.origin.x),
            lo(y0, m.origin.y),
            hi(x1, m.origin.x, m.nx),
            hi(y1, m.origin.y, m.ny),
          );
          let out = repair_region(&board, &m, &rules, &copper, window, fp);
          if out.isolated == 0 || grown >= margin + 12.0 {
            break out;
          }
          grown += 3.0;
        };
        let mut d2 = doc.clone();
        let w2 = autolayout_route::write::add_routes_fine(&mut d2, &m, &board, &out.routes)?;
        let text = d2.to_string();
        let d2 = parse(&text).map_err(|e| format!("internal: {e}"))?;
        let b2 = Board::read(&d2).map_err(|e| e.to_string())?;
        let plane_codes: Vec<(String, u32)> = m.planes.iter().map(|p| (p.layer.clone(), p.net)).collect();
        let v2 = autolayout_route::verify::verify_fresh_planes(&d2, &b2, &rules, &plane_codes);
        let better = (v2.unconnected, !v2.clean() as usize) < (v.unconnected, !v.clean() as usize);
        println!(
          "repair window {} around {} stuck region(s) near ({:.1}, {:.1}), margin {:.0} mm: {} net(s) re-routed ({} with a terminal unjoined), {} dropped, {} iteration(s), {:.1} s; verifier unconnected {} -> {}, clearance+shorting+hole-to-hole {} -> {}: {}",
          gi + 1,
          g.len(),
          (x0 + x1) / 2.0,
          (y0 + y1) / 2.0,
          grown,
          out.nets_rerouted,
          out.nets_failed,
          out.dropped,
          out.iterations,
          t.elapsed().as_secs_f64(),
          v.unconnected,
          v2.unconnected,
          v.clearance + v.shorting + v.hole_to_hole,
          v2.clearance + v2.shorting + v2.hole_to_hole,
          if better { "kept" } else { "not better, discarded" }
        );
        if std::env::var("AUTOLAYOUT_REPAIR_DIFF").is_ok() {
          for (tag, rep) in [("before", &v), ("after", &v2)] {
            for i in rep.issues.iter().filter(|i| i.kind == "unconnected") {
              println!(
                "  {tag}: {} at ({:.3}, {:.3}): {}",
                i.kind, i.at.x, i.at.y, i.detail
              );
            }
          }
        }
        if better {
          copper = out.routes;
          judged = d2;
          written = w2;
          v = v2;
          repaired = true;
        }
      }
    }
  }
  println!(
    "rules from {}; pitch {:.4} mm; grid {} x {} x {}",
    rules.source,
    m.pitch,
    m.nx,
    m.ny,
    m.layers.len()
  );
  println!(
    "{} route (UNVERIFIED: not written, not checked by KiCad; stub reach {} pitches, shared stub corridors {}, island repair rounds {repair_rounds}, tap repair rounds {tap_rounds}): nets complete {} of {}; pads connected {} of {}; {:.1} mm of track, {} vias, {:.1} s",
    if greedy {
      "greedy sequential"
    } else {
      "negotiated (PathFinder)"
    },
    chosen.0,
    chosen.1,
    s.nets_complete,
    s.nets_total,
    s.pads_connected,
    s.pads_total,
    s.length_mm,
    s.vias,
    t.elapsed().as_secs_f64()
  );
  for line in &trail {
    println!("{line}");
  }
  if let Some(st) = &stats {
    println!(
      "  {} iteration(s); contested cells per iteration {:?}; nets dropped to make the result legal {} (completed by hard re-route: {}; improved by rip-up retry: {})",
      st.iterations, st.conflicts, st.dropped_nets, st.repaired_nets, st.ripup_improved
    );
  }
  for r in s.routes.iter().filter(|r| !r.failed.is_empty()).take(12) {
    let pads: Vec<String> = r
      .failed
      .iter()
      .filter_map(|&(f, p)| {
        Some(format!(
          "{}.{}",
          board.footprints.get(f)?.reference,
          board.footprints.get(f)?.pads.get(p)?.number
        ))
      })
      .collect();
    println!(
      "  incomplete: net {} — unreached {}",
      board.nets.get(&r.net).map_or("?", String::as_str),
      pads.join(" ")
    );
  }
  if explain {
    print_why(&board, &s);
    // Failed pads the search recorded no reason for: say how many on-grid cells the pad itself has.
    for r in &s.routes {
      for &(f, p) in &r.failed {
        if r.why.iter().any(|w| w.pad == (f, p)) {
          continue;
        }
        let Some(pad) = board.footprints.get(f).and_then(|fp| fp.pads.get(p)) else {
          continue;
        };
        println!(
          "  unexplained: net {} pad {}.{} at ({:.3}, {:.3}): {} on-grid landing cells inside the pad",
          board.nets.get(&r.net).map_or("?", String::as_str),
          board.footprints.get(f).map_or("?", |fp| fp.reference.as_str()),
          pad.number,
          pad.at.x,
          pad.at.y,
          m.pad_access(&board, f, p).cells.len()
        );
      }
    }
  }
  if let Some(o) = &out_path {
    fs::write(o, judged.to_string()).map_err(|e| format!("{o}: {e}"))?;
    // Read it back off disk: it must parse, round-trip byte-exact, and hold
    // exactly the copper that was added.
    let back = fs::read_to_string(o).map_err(|e| format!("{o}: {e}"))?;
    let parsed = parse(&back).map_err(|e| format!("{o}: written file does not parse: {e}"))?;
    if parsed.to_string() != back {
      return Err(format!("{o}: written file does not round-trip byte-exact"));
    }
    let count = |d: &Document, k: &str| d.root().map_or(0, |r| r.children(k).count());
    let (_, before) = load(pcb)?;
    let added_seg = count(&parsed, "segment") - count(&before, "segment");
    let added_via = count(&parsed, "via") - count(&before, "via");
    println!(
      "wrote {o}: {} segments, {} vias added (read back: {added_seg} / {added_via}); judge it with KiCad DRC, not with this tool",
      written.segments, written.vias
    );
    if (added_seg, added_via) != (written.segments, written.vias) {
      return Err("read-back copper count differs from what was written".to_owned());
    }
  }
  println!(
    "verifier (re-derived from the written copper, NOT KiCad; judge is `kicad-cli pcb drc`): {}",
    v.summary()
  );
  for i in v.issues.iter().take(10) {
    println!("  {} at ({:.4}, {:.4}): {}", i.kind, i.at.x, i.at.y, i.detail);
  }
  if repaired {
    return Ok(v.unconnected == 0 && v.clean());
  }
  Ok(s.nets_complete == s.nets_total && v.clean())
}

/// Parts with a pad this close to a stuck region are listed on its `STUCK` line.
const STUCK_NEIGHBOUR_MM: f64 = 3.0;

/// `route --explain`: for each pad the kept attempt left unreached, which end of its net is boxed in
/// and what blocks it (see `search::Why`).
fn print_why(board: &Board, s: &autolayout_route::driver::Summary) {
  let name = |(f, p): (usize, usize)| {
    board
      .footprints
      .get(f)
      .and_then(|fp| Some(format!("{}.{}", fp.reference, fp.pads.get(p)?.number)))
      .unwrap_or_else(|| "?".into())
  };
  println!("why unreached (the end that reaches fewer grid cells is the boxed-in one):");
  for r in &s.routes {
    for w in &r.why {
      let (side, reach, other, block) = if w.tree_reach <= w.pad_reach {
        (
          format!(
            "joined end [{}]",
            w.tree_pads.iter().map(|&p| name(p)).collect::<Vec<_>>().join(" ")
          ),
          w.tree_reach,
          w.pad_reach,
          w.tree_block,
        )
      } else {
        (
          format!("pad {} itself", name(w.pad)),
          w.pad_reach,
          w.tree_reach,
          w.pad_block,
        )
      };
      let verdict = if reach.max(other) > 2000 && reach > 300 {
        "neither end boxed in: the regions do not meet"
      } else {
        "boxed in"
      };
      println!(
        "  net {} pad {}: {verdict}; {side} reaches {reach} cells (other end {other}); blocked by fixed obstacles {}, plane claims {}, other nets {}",
        board.nets.get(&r.net).map_or("?", String::as_str),
        name(w.pad),
        block.0,
        block.1,
        block.2
      );
      if verdict == "boxed in" {
        // One machine-readable line per stuck region for the placement feedback loop
        // (`tools/autoplace.py`): where it is, and which parts have a pad within 3 mm of it.
        let at = if w.tree_reach <= w.pad_reach {
          w.tree_at
        } else {
          w.pad_at
        };
        let near: Vec<&str> = board
          .footprints
          .iter()
          .filter(|fp| fp.pads.iter().any(|p| (p.at - at).length() <= STUCK_NEIGHBOUR_MM))
          .map(|fp| fp.reference.as_str())
          .collect();
        println!("STUCK\t{:.3}\t{:.3}\t{}", at.x, at.y, near.join(","));
      }
      if std::env::var("AUTOLAYOUT_MAPS").is_ok() {
        print!(
          "{}",
          if w.tree_reach <= w.pad_reach {
            &w.tree_map
          } else {
            &w.pad_map
          }
        );
      }
    }
  }
}

/// `autolayout route PCB --fine [--sub N] [--fine-iters N] [--via-cost F] [--fine-slack MM] [--fine-seed S]`:
/// the fine-lattice router (`route::fine`) on every net. A board that already has
/// copper or keepout zones is refused (the fine router does not read them yet).
fn route_fine_cmd(
  pcb: &str,
  doc: &Document,
  board: &Board,
  rules: &autolayout_route::rules::DesignRules,
  m: &autolayout_route::model::RoutingModel,
  out_path: Option<&str>,
  params: autolayout_route::fine::FineParams,
) -> Result<bool, String> {
  let st = &m.stats;
  if st.existing_segments + st.existing_vias + st.keepout_zones > 0 {
    return Err(format!(
      "{pcb}: --fine does not read existing copper or keepout zones yet ({} tracks, {} vias, {} keepouts); route a stripped board",
      st.existing_segments, st.existing_vias, st.keepout_zones
    ));
  }
  let t = std::time::Instant::now();
  let res = autolayout_route::fine::route_fine(board, m, rules, params);
  let mut judged = doc.clone();
  let written = autolayout_route::write::add_routes_fine(&mut judged, m, board, &res.routes)?;
  let jtext = judged.to_string();
  let jdoc = parse(&jtext).map_err(|e| format!("internal: {e}"))?;
  let jboard = Board::read(&jdoc).map_err(|e| e.to_string())?;
  let plane_codes: Vec<(String, u32)> = m.planes.iter().map(|p| (p.layer.clone(), p.net)).collect();
  let v = autolayout_route::verify::verify_fresh_planes(&jdoc, &jboard, rules, &plane_codes);
  let mm: f64 = res
    .routes
    .iter()
    .flat_map(|r| &r.segs)
    .map(|(_, a, b)| (*b - *a).length())
    .sum();
  println!(
    "rules from {}; fine lattice {} x {} cells of {:.4} mm (grid pitch {:.4} / {}), {} signal layer(s), seed {}",
    rules.source,
    res.lattice.0,
    res.lattice.1,
    res.step,
    m.pitch,
    params.sub,
    m.layers.len(),
    params.seed
  );
  println!(
    "fine route (UNVERIFIED: not checked by KiCad): nets complete {} of {}; {:.1} mm of track, {} vias, {} iteration(s), {} net(s) dropped for overlap, {:.1} s",
    res.nets_complete,
    res.nets_total,
    mm,
    written.vias,
    res.iterations,
    res.dropped,
    t.elapsed().as_secs_f64()
  );
  for r in res.routes.iter().filter(|r| !r.failed.is_empty()).take(20) {
    let pads: Vec<String> = r
      .failed
      .iter()
      .filter_map(|&(f, p)| {
        Some(format!(
          "{}.{}",
          board.footprints.get(f)?.reference,
          board.footprints.get(f)?.pads.get(p)?.number
        ))
      })
      .collect();
    println!(
      "  incomplete: net {} \u{2014} unreached {}",
      board.nets.get(&r.net).map_or("?", String::as_str),
      pads.join(" ")
    );
  }
  if let Some(o) = out_path {
    fs::write(o, judged.to_string()).map_err(|e| format!("{o}: {e}"))?;
    let back = fs::read_to_string(o).map_err(|e| format!("{o}: {e}"))?;
    let parsed = parse(&back).map_err(|e| format!("{o}: written file does not parse: {e}"))?;
    if parsed.to_string() != back {
      return Err(format!("{o}: written file does not round-trip byte-exact"));
    }
    println!(
      "wrote {o}: {} segments, {} vias added; judge it with KiCad DRC, not with this tool",
      written.segments, written.vias
    );
  }
  println!(
    "verifier (re-derived from the written copper, NOT KiCad; judge is `kicad-cli pcb drc`): {}",
    v.summary()
  );
  for i in v.issues.iter().take(10) {
    println!("  {} at ({:.4}, {:.4}): {}", i.kind, i.at.x, i.at.y, i.detail);
  }
  Ok(res.nets_complete == res.nets_total && v.clean())
}

/// Judge the copper already in a board file against its project's rules, in
/// continuous geometry from the file alone (see `route::verify`). `--plane
/// LAYER=NET` says which layers are poured planes. Exit 1 if anything is found.
fn verify(pcb: &str, args: &[String]) -> Result<bool, String> {
  // `--probe X,Y` (repeatable): which copper and which part of its net lies there.
  let mut probes = Vec::new();
  let mut rest = Vec::new();
  let mut it = args.iter();
  while let Some(a) = it.next() {
    if a == "--probe" {
      let v = it.next().ok_or("--probe needs X,Y")?;
      let (x, y) = v.split_once(',').ok_or("--probe needs X,Y")?;
      let num = |s: &str| {
        s.trim()
          .parse::<f64>()
          .ok()
          .filter(|n| n.is_finite())
          .ok_or_else(|| format!("not a number: {s}"))
      };
      probes.push(autolayout_board::Point::new(num(x)?, num(y)?));
    } else {
      rest.push(a.clone());
    }
  }
  let (rules, _, planes) = model_args(pcb, &rest)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let codes: Vec<(String, u32)> = planes
    .iter()
    .map(|(l, n)| {
      board
        .nets
        .iter()
        .find(|(_, name)| *name == n)
        .map(|(c, _)| (l.clone(), *c))
        .ok_or_else(|| format!("plane net `{n}` is not a net of this board"))
    })
    .collect::<Result<_, _>>()?;
  let v = autolayout_route::verify::verify_with_probes(&doc, &board, &rules, &codes, &probes);
  println!("{pcb}: {}", v.summary());
  for i in v.issues.iter().take(400) {
    println!("  {} at ({:.4}, {:.4}): {}", i.kind, i.at.x, i.at.y, i.detail);
  }
  for n in &v.probe_notes {
    println!("{n}");
  }
  Ok(v.clean())
}

/// Print which nets would take the inner layers as planes, chosen from the
/// board's own pad counts (see `route::planes`), plus ready-made flags.
fn planes(pcb: &str) -> Result<bool, String> {
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let p = autolayout_route::planes::plan(&doc, &board);
  println!(
    "{pcb}: {} copper layer(s): {}",
    p.copper.len(),
    p.copper.join(" ")
  );
  for (layer, net, pads) in &p.assignments {
    println!("  plane {layer} = {net} ({pads} pads)");
  }
  for layer in &p.unassigned {
    println!(
      "  {layer}: no net with {}+ pads; no plane",
      autolayout_route::planes::PLANE_MIN_PADS
    );
  }
  let flags: Vec<String> = p
    .assignments
    .iter()
    .map(|(l, n, _)| format!("--plane\t{l}={n}"))
    .collect();
  println!("FLAGS\t{}", flags.join("\t"));
  Ok(true)
}

/// Build the routing model for one net class and report what it blocks and
/// which netted pads it can reach (M7.1). Nothing is written.
/// `autolayout escape-survey PCB [rule flags]`: for every SMD pad with a net,
/// the longest straight track along its axis that keeps the clearance to every
/// foreign pad (continuous geometry, `route::survey`). One line per pad:
/// `REF.PAD net=NAME best=MM dir=X,Y`.
fn escape_survey(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::survey::axis_room;
  let (rules, profile, _) = model_args(pcb, args)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let margin = rules.min.max_error;
  let clearance = rules
    .classes
    .iter()
    .map(|c| c.clearance)
    .fold(profile.clearance.max(rules.min.clearance), f64::max);
  for (fi, f) in board.footprints.iter().enumerate() {
    for (pi, p) in f.pads.iter().enumerate() {
      if let Some(r) = axis_room(&board, profile.track_width, clearance, margin, fi, pi, 3.0) {
        println!(
          "{}.{} net={} best={:.2} dir={:.0},{:.0}",
          f.reference,
          p.number,
          p.net.and_then(|n| board.nets.get(&n)).map_or("?", String::as_str),
          r.best,
          r.dir.0,
          r.dir.1
        );
      }
    }
  }
  Ok(true)
}

/// `autolayout capacity PCB [--part REF] [--min-pads N] [--all] [--plane L=N ...] [rule flags]`:
/// for each part with at least N netted pads (default 8), nets that must cross a ring around
/// it against the tracks the ring can pass, at several distances (`route::capacity`).
/// Parts with a tight or over-capacity ring are listed; `--all` lists every part.
fn capacity(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::capacity::{CapacityRules, ring_reports};
  use autolayout_route::model::RoutingModel;
  let (mut part, mut min_pads, mut all) = (None::<String>, 8usize, false);
  let mut rest = Vec::new();
  let mut it = args.iter();
  while let Some(a) = it.next() {
    match a.as_str() {
      "--part" => part = Some(it.next().ok_or("--part needs a reference")?.clone()),
      "--min-pads" => {
        min_pads = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--min-pads needs a number")?;
      }
      "--all" => all = true,
      _ => rest.push(a.clone()),
    }
  }
  let (rules, profile, planes) = model_args(pcb, &rest)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).map_err(|e| e.to_string())?;
  let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
  let clearance = rules
    .classes
    .iter()
    .map(|c| c.clearance)
    .fold(profile.clearance.max(rules.min.clearance), f64::max);
  let cr = CapacityRules {
    track: profile.track_width,
    clearance,
    margin: rules.min.max_error,
    board_edge: rules.min.copper_edge_clearance,
  };
  println!(
    "track {:.4} clearance {:.4}: a track needs {:.4} mm from any copper edge, two tracks {:.4} mm centre to centre; signal layers {}",
    cr.track,
    cr.clearance,
    cr.edge(),
    cr.pitch(),
    m.layers.join(", ")
  );
  let offsets = [0.6, 1.0, 1.5, 2.0, 3.0, 4.5, 6.0];
  let mut worst = 0usize;
  for (fi, f) in board.footprints.iter().enumerate() {
    let netted = f.pads.iter().filter(|p| p.net.is_some_and(|n| n != 0)).count();
    let wanted = match &part {
      Some(r) => &f.reference == r,
      None => netted >= min_pads,
    };
    if !wanted {
      continue;
    }
    let reps = ring_reports(&board, fi, &offsets, &cr, &m.layers, &plane_nets, board.outline);
    let flag = |r: &autolayout_route::capacity::RingReport| {
      if r.demand() > r.capacity_loose() {
        "IMPOSSIBLE"
      } else if r.demand() > r.capacity_strict() {
        "OVER"
      } else if r.demand() * 10 > r.capacity_strict() * 8 {
        "tight"
      } else {
        ""
      }
    };
    let bad = reps.iter().any(|r| !flag(r).is_empty());
    if !(all || part.is_some() || bad) {
      continue;
    }
    worst += reps
      .iter()
      .filter(|r| matches!(flag(r), "IMPOSSIBLE" | "OVER"))
      .count();
    println!("{} ({} netted pads):", f.reference, netted);
    for r in &reps {
      println!(
        "  ring +{:.1} mm: {:3} nets must cross, room for {:3} (strict) / {:3} (loose), per layer {:?}  {}",
        r.offset,
        r.demand(),
        r.capacity_strict(),
        r.capacity_loose(),
        r.strict,
        flag(r)
      );
    }
  }
  Ok(worst == 0)
}

/// `autolayout congestion PCB [--layers N] [--max-layers M] [--tile MM] [--plane L=N ...] [rule flags]`:
/// tile-level global routing (`route::congestion`): for each signal layer count, the tracks and
/// vias the board's nets need beyond what its free space holds. Zero overflow is necessary, not
/// sufficient, for the board to route on that many layers.
fn congestion(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::capacity::CapacityRules;
  use autolayout_route::congestion::{CongestionParams, predict};
  use autolayout_route::model::RoutingModel;
  let (mut layers, mut max_layers, mut params) = (None::<usize>, 4usize, CongestionParams::default());
  let mut rest = Vec::new();
  let mut it = args.iter();
  while let Some(a) = it.next() {
    let mut num = |what: &str| -> Result<String, String> {
      it.next().cloned().ok_or_else(|| format!("{what} needs a value"))
    };
    match a.as_str() {
      "--layers" => layers = Some(num(a)?.parse().map_err(|_| "--layers needs a number")?),
      "--max-layers" => max_layers = num(a)?.parse().map_err(|_| "--max-layers needs a number")?,
      "--tile" => params.tile = num(a)?.parse().map_err(|_| "--tile needs a number")?,
      _ => rest.push(a.clone()),
    }
  }
  let (rules, profile, planes) = model_args(pcb, &rest)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).map_err(|e| e.to_string())?;
  let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
  let clearance = rules
    .classes
    .iter()
    .map(|c| c.clearance)
    .fold(profile.clearance.max(rules.min.clearance), f64::max);
  let cr = CapacityRules {
    track: profile.track_width,
    clearance,
    margin: rules.min.max_error,
    board_edge: rules.min.copper_edge_clearance,
  };
  let counts: Vec<usize> = match layers {
    Some(n) => vec![n],
    None => (1..=max_layers.max(1)).collect(),
  };
  println!(
    "tile {:.2} mm; board signal layers {} ({}); via {:.4}, via spacing {:.4}; {} plane net(s)",
    params.tile,
    m.layers.len(),
    m.layers.join(", "),
    profile.via_diameter,
    m.via_spacing,
    plane_nets.len()
  );
  let preds = predict(
    &board,
    &cr,
    (
      profile.via_diameter,
      m.via_spacing.max(profile.via_diameter + clearance),
    ),
    &m.layers,
    &plane_nets,
    &counts,
    params,
  );
  let mut first_ok: Option<usize> = None;
  for p in &preds {
    if p.overflow == 0 && first_ok.is_none() {
      first_ok = Some(p.layers);
    }
    println!(
      "{} signal layer(s): overflow {:4} ({} edges, {} via tiles; per layer {:?}); layer changes {} of {} via sites; {} nets, {} iteration(s)",
      p.layers,
      p.overflow,
      p.overflow_edges,
      p.overflow_via_tiles,
      p.per_layer_overflow,
      p.vias_used,
      p.via_capacity,
      p.nets,
      p.iterations
    );
    for h in p.hotspots.iter().take(5) {
      println!(
        "    ({:.1}, {:.1}) {}{}: uses {} of {}",
        h.at.x,
        h.at.y,
        h.what,
        h.layer.map_or(String::new(), |l| format!(" on layer {l}")),
        h.usage,
        h.capacity
      );
    }
  }
  match first_ok {
    Some(n) => {
      println!("predicted: {n} signal layer(s) leave no overflow in this model (necessary, not sufficient)")
    }
    None => println!(
      "predicted: more than {} signal layers, or more room between parts",
      counts.last().copied().unwrap_or(0)
    ),
  }
  Ok(first_ok.is_some())
}

/// `autolayout place PCB NET OUT [--no-halo] [--grade] [--pairs FILE]`.
fn place_cmd(pcb: &str, net: &str, out: &str, flags: &[String]) -> Result<bool, String> {
  // --no-halo reproduces M4.1 exactly (and keeps parts on the side they started on: front-only is
  // the default policy otherwise). Spreading is off by default; --spread turns it on.
  let (mut halo, mut pairs, mut grade, mut spread, mut halo_scale) =
    (true, None::<&String>, false, false, 1.0f64);
  let mut boost = None::<&String>;
  let mut it = flags.iter();
  while let Some(f) = it.next() {
    match f.as_str() {
      "--no-halo" => halo = false,
      "--grade" => grade = true,
      "--spread" => spread = true,
      "--halo-scale" => {
        halo_scale = it
          .next()
          .and_then(|v| v.parse().ok())
          .filter(|v: &f64| v.is_finite() && *v >= 0.0)
          .ok_or("--halo-scale needs a number >= 0")?
      }
      "--pairs" => pairs = Some(it.next().ok_or("--pairs needs a file")?),
      "--halo-boost" => boost = Some(it.next().ok_or("--halo-boost needs a file")?),
      other => return Err(format!("place: unknown flag `{other}`")),
    }
  }
  let opts = autolayout_place::place::Options {
    escape_halo: halo,
    spread,
    front_only: halo,
    grade,
    halo_scale,
  };
  place(pcb, net, out, opts, pairs, boost)
}

/// `autolayout grades PCB [--top N]`: each part's connection grade (the number of other parts it
/// shares a signal net with, `place::grade`), highest first, with its strongest neighbours.
fn grades_cmd(pcb: &str, args: &[String]) -> Result<bool, String> {
  let mut top = 25usize;
  let mut it = args.iter();
  while let Some(a) = it.next() {
    match a.as_str() {
      "--top" => {
        top = it
          .next()
          .and_then(|v| v.parse().ok())
          .ok_or("--top needs a number")?
      }
      other => return Err(format!("grades: unknown flag `{other}`")),
    }
  }
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let g = autolayout_place::grade::grades(&board, autolayout_place::place::PLANE_FANOUT);
  let mut order: Vec<usize> = (0..board.footprints.len()).collect();
  order.sort_by(|&a, &b| g.grade[b].cmp(&g.grade[a]).then(a.cmp(&b)));
  let mut hist: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
  for &gr in &g.grade {
    *hist.entry(gr).or_default() += 1;
  }
  println!("{} parts; parts per grade: {:?}", board.footprints.len(), hist);
  for &f in order.iter().take(top) {
    let Some(fp) = board.footprints.get(f) else {
      continue;
    };
    let mut near: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    for l in g.links.get(f).map_or(&[][..], Vec::as_slice) {
      *near.entry(l.other).or_default() += 1;
    }
    let mut near: Vec<(usize, usize)> = near.into_iter().collect();
    near.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let names: Vec<String> = near
      .iter()
      .take(6)
      .filter_map(|&(o, k)| Some(format!("{}x{k}", board.footprints.get(o)?.reference)))
      .collect();
    println!(
      "  {:<8} grade {:3}  {} pads  links {}",
      fp.reference,
      g.grade[f],
      fp.pads.len(),
      names.join(" ")
    );
  }
  Ok(true)
}

/// `autolayout escape-plan PCB --part REF --out NEW.kicad_pcb [--margin MM] [--sub N] [--iters N]
/// [--toward MM] [--slack MM] [--hist X] [--plane L=N ...] [rule flags]`: plan the way out of one
/// part (`fine::plan_escapes`): every pad on a signal net to the edge of a window around the part,
/// every plane-net pad to a legal via, as copper in a new file, so `route` can start from it.
fn escape_plan(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::fine::{EscapeParams, FineParams, plan_escapes};
  use autolayout_route::model::RoutingModel;
  let (mut part, mut out_path) = (None::<String>, None::<String>);
  let mut esc = EscapeParams {
    part: 0,
    margin: 1.8,
    toward: None,
  };
  let mut fp = FineParams {
    sub: 12,
    iters: 250,
    slack: -0.003,
    hist_inc: 1.0,
    ..FineParams::default()
  };
  let mut rest = Vec::new();
  let mut it = args.iter();
  while let Some(a) = it.next() {
    let mut val = |what: &str| -> Result<String, String> {
      it.next().cloned().ok_or_else(|| format!("{what} needs a value"))
    };
    match a.as_str() {
      "--part" => part = Some(val(a)?),
      "--out" => out_path = Some(val(a)?),
      "--margin" => esc.margin = val(a)?.parse().map_err(|_| "--margin needs a number")?,
      "--toward" => esc.toward = Some(val(a)?.parse().map_err(|_| "--toward needs a number")?),
      "--sub" => fp.sub = val(a)?.parse().map_err(|_| "--sub needs a number")?,
      "--iters" => fp.iters = val(a)?.parse().map_err(|_| "--iters needs a number")?,
      "--slack" => fp.slack = val(a)?.parse().map_err(|_| "--slack needs a number")?,
      "--hist" => fp.hist_inc = val(a)?.parse().map_err(|_| "--hist needs a number")?,
      _ => rest.push(a.clone()),
    }
  }
  let part = part.ok_or("escape-plan needs --part REF")?;
  let out_path = out_path.ok_or("escape-plan needs --out NEW.kicad_pcb")?;
  let same = fs::canonicalize(&out_path)
    .ok()
    .zip(fs::canonicalize(pcb).ok())
    .is_some_and(|(a, b)| a == b);
  if same || Path::new(&out_path).exists() {
    return Err(format!(
      "{out_path}: already exists or is the input; refusing to overwrite"
    ));
  }
  let (rules, profile, planes) = model_args(pcb, &rest)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  esc.part = board
    .footprints
    .iter()
    .position(|f| f.reference == part)
    .ok_or_else(|| format!("no part `{part}` on the board"))?;
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).map_err(|e| e.to_string())?;
  let st = &m.stats;
  if st.existing_segments + st.existing_vias + st.keepout_zones > 0 {
    return Err(format!(
      "{pcb}: escape-plan does not read existing copper or keepout zones ({} tracks, {} vias, {} keepouts); plan on a stripped board",
      st.existing_segments, st.existing_vias, st.keepout_zones
    ));
  }
  let t = std::time::Instant::now();
  let res = plan_escapes(&board, &m, &rules, fp, esc);
  let mut judged = doc.clone();
  let written = autolayout_route::write::add_routes_fine(&mut judged, &m, &board, &res.routes)?;
  let mm: f64 = res
    .routes
    .iter()
    .flat_map(|r| &r.segs)
    .map(|(_, a, b)| (*b - *a).length())
    .sum();
  println!(
    "escape plan for {part}: window lattice {} x {} cells of {:.4} mm, sub {}, {} iteration(s), {:.1} s",
    res.lattice.0,
    res.lattice.1,
    res.step,
    fp.sub,
    res.iterations,
    t.elapsed().as_secs_f64()
  );
  println!(
    "nets complete {} of {} ({} dropped for overlap); {:.1} mm of track, {} vias (UNVERIFIED: not checked by KiCad)",
    res.nets_complete, res.nets_total, res.dropped, mm, written.vias
  );
  for r in res.routes.iter().filter(|r| !r.failed.is_empty()).take(20) {
    let pads: Vec<String> = r
      .failed
      .iter()
      .filter_map(|&(f, p)| {
        Some(format!(
          "{}.{}",
          board.footprints.get(f)?.reference,
          board.footprints.get(f)?.pads.get(p)?.number
        ))
      })
      .collect();
    println!(
      "  no escape: net {} \u{2014} {}",
      board.nets.get(&r.net).map_or("?", String::as_str),
      pads.join(" ")
    );
  }
  fs::write(&out_path, judged.to_string()).map_err(|e| format!("{out_path}: {e}"))?;
  let back = fs::read_to_string(&out_path).map_err(|e| format!("{out_path}: {e}"))?;
  let parsed = parse(&back).map_err(|e| format!("{out_path}: written file does not parse: {e}"))?;
  if parsed.to_string() != back {
    return Err(format!("{out_path}: written file does not round-trip byte-exact"));
  }
  println!("wrote {out_path}");
  Ok(res.nets_complete == res.nets_total)
}

fn model(pcb: &str, args: &[String]) -> Result<bool, String> {
  use autolayout_route::model::RoutingModel;
  let (rules, profile, planes) = model_args(pcb, args)?;
  let (_, doc) = load(pcb)?;
  let board = Board::read(&doc).map_err(|e| format!("{pcb}: {e}"))?;
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).map_err(|e| e.to_string())?;
  print!("{}", rules.report());
  println!(
    "profile `{}`: track {:.4} clearance {:.4} via {:.4}/{:.4}; pitch {:.4} mm",
    profile.name, profile.track_width, profile.clearance, profile.via_diameter, profile.via_drill, m.pitch
  );
  println!(
    "grid {} x {} cells, {} routable layer(s)",
    m.nx,
    m.ny,
    m.layers.len()
  );
  for (i, l) in m.layers.iter().enumerate() {
    println!(
      "  {l:<8} {:5.1} % of cells unusable by a typical net",
      100.0 * m.blocked_fraction(i)
    );
  }
  for p in &m.planes {
    println!(
      "  plane {} = {}",
      p.layer,
      board.nets.get(&p.net).map_or("?", String::as_str)
    );
  }
  let sx = &m.stats;
  println!(
    "copper already on board: {} segments/arcs, {} vias (kept as obstacles); keepout zones used {}; zone fills ignored {}; custom pads approximated {}",
    sx.existing_segments, sx.existing_vias, sx.keepout_zones, sx.ignored_fills, sx.custom_pads
  );
  let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
  let (mut total, mut none, mut sample) = (0usize, 0usize, Vec::new());
  let (mut by_kind, mut cells_sum) = (BTreeMap::<String, (usize, usize)>::new(), 0usize);
  for (fi, fp) in board.footprints.iter().enumerate() {
    for (pi, pad) in fp.pads.iter().enumerate() {
      let Some(net) = pad.net.filter(|&n| n != 0) else {
        continue;
      };
      if plane_nets.contains(&net) && pad.kind == "smd" {
        continue;
      }
      total += 1;
      let a = m.pad_access(&board, fi, pi);
      cells_sum += a.cells.len();
      let e = by_kind.entry(pad.kind.clone()).or_default();
      e.0 += 1;
      if a.cells.is_empty() {
        none += 1;
        e.1 += 1;
        if sample.len() < 12 {
          sample.push(format!("{}.{} ({})", fp.reference, pad.number, pad.shape));
        }
      }
    }
  }
  println!(
    "pad access (netted pads, plane-net SMD pads excluded): {} of {total} have no usable grid cell",
    none
  );
  for (k, (n, bad)) in &by_kind {
    println!("  {k:<14} {n:5} pads, {bad:4} unreachable");
  }
  if total > 0 {
    println!(
      "  mean access cells per pad {:.2}",
      cells_sum as f64 / total as f64
    );
  }
  if !sample.is_empty() {
    println!("  e.g. {}", sample.join(", "));
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
    Some((cmd, [input, output, flag, opts @ ..])) if cmd == "flip" && flag == "--small-caps" => {
      flip_command(input, output, opts)
    }
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
    Some((cmd, [pcb, net, out, flags @ ..])) if cmd == "place" => place_cmd(pcb, net, out, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "grades" => grades_cmd(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "rules" => rules(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "model" => model(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "congestion" => congestion(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "escape-plan" => escape_plan(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "capacity" => capacity(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "escape-survey" => escape_survey(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "route" => route(pcb, flags),
    Some((cmd, [pcb, flags @ ..])) if cmd == "verify" => verify(pcb, flags),
    Some((cmd, [pcb])) if cmd == "planes" => planes(pcb),
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
