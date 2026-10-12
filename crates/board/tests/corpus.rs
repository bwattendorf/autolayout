//! Pad positions checked against copper a router actually laid.
//!
//! On a routed board, tracks and vias end on pad centres. So for each pad with
//! copper on its net, "is there a track end or via within 1 µm of where we
//! computed the pad?" grades the footprint → board transform without trusting
//! it. Two deliberately wrong transforms are graded the same way; the test
//! fails unless ours wins clearly on rotated parts and on back-side parts,
//! which are exactly where a sign error or a missing mirror would hide.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use autolayout_board::{Board, Footprint, Point, Side};
use autolayout_sexpr::parse;

/// Routed boards in the corpus, relative to `AUTOLAYOUT_CORPUS`.
const ROUTED: [&str; 2] = ["quilter/fab/m1/m1.kicad_pcb", "quilter/fab/emu/emu.kicad_pcb"];
const TOL_MM: f64 = 0.001;

#[test]
fn pad_centres_meet_routed_copper() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  for rel in ROUTED {
    let path = Path::new(&root).join(rel);
    let src = fs::read_to_string(&path).unwrap();
    let doc = parse(&src).unwrap();
    let board = Board::read(&doc).unwrap();
    let ends = copper_ends(&doc);

    let ours = score(&board, &ends, |fp, local| fp.to_board(local));
    let clockwise = score(&board, &ends, |fp, local| local.rotated(-fp.rotation) + fp.at);
    let mirrored = score(&board, &ends, |fp, local| {
      let l = if fp.side == Side::Back {
        Point::new(-local.x, local.y)
      } else {
        local
      };
      l.rotated(fp.rotation) + fp.at
    });
    eprintln!("{rel}");
    eprintln!("  ours           {ours}");
    eprintln!("  wrong: CW rot  {clockwise}");
    eprintln!("  wrong: mirror  {mirrored}");

    for (name, s) in [("rotated front", ours.rotated_front), ("back", ours.back)] {
      assert!(s.total >= 20, "{rel}: too few {name} pads to judge ({})", s.total);
      assert!(
        s.rate() > 0.9,
        "{rel}: only {:.1}% of {name} pads meet copper",
        100.0 * s.rate()
      );
    }
    assert!(
      ours.rotated_front.rate() > clockwise.rotated_front.rate() + 0.3,
      "{rel}: rotation sense not distinguished"
    );
    assert!(
      ours.back.rate() > mirrored.back.rate() + 0.3,
      "{rel}: back-side mirroring not distinguished"
    );
  }
}

/// Chip resistors and capacitors have both pads at 0° in their library
/// footprint. If KiCad stores pad angles in board terms, every such pad on
/// every corpus board reads exactly its footprint's rotation; if it stores
/// them relative to the footprint, they all read 0. The write side depends on
/// which, so it is measured here rather than assumed.
#[test]
fn pad_angles_are_stored_in_board_terms() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let mut files = Vec::new();
  collect(Path::new(&root), &mut files);
  let (mut absolute, mut relative, mut rotated_pads) = (0usize, 0usize, 0usize);
  for path in &files {
    let doc = parse(&fs::read_to_string(path).unwrap()).unwrap();
    let board = Board::read(&doc).unwrap();
    for fp in &board.footprints {
      let chip = fp.lib_id.starts_with("Resistor_SMD:R_") || fp.lib_id.starts_with("Capacitor_SMD:C_");
      if !chip || (fp.rotation % 360.0).abs() < 1e-6 {
        continue;
      }
      for pad in &fp.pads {
        rotated_pads += 1;
        if same_angle(pad.angle, fp.rotation) {
          absolute += 1;
        } else if same_angle(pad.angle, 0.0) {
          relative += 1;
        }
      }
    }
  }
  eprintln!(
    "pad angles on rotated chip R/C across {} files: {absolute} board-terms, {relative} footprint-relative, of {rotated_pads}",
    files.len()
  );
  assert!(rotated_pads > 1000, "too few rotated chip pads to judge");
  assert_eq!(
    absolute, rotated_pads,
    "some pad angles are not footprint rotation"
  );
}

fn same_angle(a: f64, b: f64) -> bool {
  ((a - b).rem_euclid(360.0) + 1e-6).rem_euclid(360.0) < 2e-6
}

fn collect(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
  let Ok(entries) = fs::read_dir(dir) else { return };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      collect(&path, out);
    } else if path.extension().is_some_and(|e| e == "kicad_pcb") {
      out.push(path);
    }
  }
}

#[derive(Default, Clone, Copy)]
struct Tally {
  hit: usize,
  total: usize,
}

#[derive(Default)]
struct Score {
  rotated_front: Tally,
  back: Tally,
  all: Tally,
}

impl Tally {
  fn rate(self) -> f64 {
    if self.total == 0 {
      0.0
    } else {
      self.hit as f64 / self.total as f64
    }
  }
}

impl std::fmt::Display for Score {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let t = |t: Tally| format!("{}/{} ({:.1}%)", t.hit, t.total, 100.0 * t.rate());
    write!(
      f,
      "all {}  rotated-front {}  back {}",
      t(self.all),
      t(self.rotated_front),
      t(self.back)
    )
  }
}

/// Every segment end and via centre, grouped by net code.
fn copper_ends(doc: &autolayout_sexpr::Document) -> HashMap<u32, Vec<Point>> {
  let mut ends: HashMap<u32, Vec<Point>> = HashMap::new();
  let root = doc.root().unwrap();
  let pt = |l: &autolayout_sexpr::List, k: &str| {
    l.child(k)
      .map(|p| Point::new(p.number(1).unwrap(), p.number(2).unwrap()))
  };
  for item in root.items.iter().filter_map(|n| n.as_list()) {
    let Some(net) = item.child("net").and_then(|n| n.number(1)) else {
      continue;
    };
    let net = net as u32;
    if item.is("segment") || item.is("arc") {
      ends
        .entry(net)
        .or_default()
        .extend([pt(item, "start").unwrap(), pt(item, "end").unwrap()]);
    } else if item.is("via") {
      ends.entry(net).or_default().push(pt(item, "at").unwrap());
    }
  }
  ends
}

fn score(
  board: &Board,
  ends: &HashMap<u32, Vec<Point>>,
  place: impl Fn(&Footprint, Point) -> Point,
) -> Score {
  let mut s = Score::default();
  for fp in &board.footprints {
    let rotated = (fp.rotation % 180.0).abs() > 1e-6;
    for pad in &fp.pads {
      let Some(copper) = pad.net.and_then(|n| ends.get(&n)) else {
        continue;
      };
      let at = place(fp, pad.local);
      let hit = copper.iter().any(|&e| (e - at).length() < TOL_MM);
      let bump = |t: &mut Tally| {
        t.total += 1;
        t.hit += usize::from(hit);
      };
      bump(&mut s.all);
      if fp.side == Side::Back {
        bump(&mut s.back);
      } else if rotated {
        bump(&mut s.rotated_front);
      }
    }
  }
  s
}
