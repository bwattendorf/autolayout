//! Typed, read-only view over a parsed KiCad board, plus the netlist that
//! carries what the board file does not.
//!
//! Reading never edits the tree: [`Board`] holds values extracted from it and
//! the index of each footprint's node, so a later write can touch that node
//! alone.
//!
//! **Pin electrical types come from the netlist, not the board.** KiCad writes
//! `(pintype …)` on pads only when the board was updated from a schematic in
//! pcbnew; boards imported by script (the whole Portcullis corpus, measured
//! 2026-10-12: zero `pintype` tokens in 116 files) carry none. So anything that
//! must decide by electrical type (CLAUDE.md rule 2) loads a [`Netlist`] beside
//! the board and joins on (reference, pad number).
//!
//! Format reference: <https://dev-docs.kicad.org/en/file-formats/sexpr-pcb/>
//! (footprint, pad, net, gr_* sections) and
//! <https://dev-docs.kicad.org/en/file-formats/sexpr-intro/> (coordinates in
//! mm, angles in degrees).

use std::collections::{BTreeMap, HashMap};

use autolayout_sexpr::{Document, List};

pub mod edit;
mod geom;
mod netlist;
pub mod parity;

pub use edit::{EditError, flip, set_pose};
pub use geom::{BBox, Point, Seg, arc_points, outlines_overlap, outlines_within};
pub use netlist::{Netlist, NetlistError};

/// A polygon primitive of a `custom` pad: points relative to the pad centre,
/// before the pad's rotation, drawn with a stroke of `width` (and filled if `fill`).
#[derive(Debug, Clone, PartialEq)]
pub struct PolyPrim {
  pub pts: Vec<Point>,
  pub width: f64,
  pub fill: bool,
}

/// Copper side a footprint is mounted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
  Front,
  Back,
}

/// How a board file refers to nets.
///
/// KiCad 9 and earlier: a top-level table `(net 1 "GND")` and references by
/// code, `(net 1 "GND")` on pads and `(net 1)` on tracks, vias and zones.
/// KiCad 10 (measured on a file it saved, version 20260206, 2026-10-16): no
/// table, no code 0 entry, and every reference is by name, `(net "GND")`, on
/// pads, tracks, vias and zones. Format reference for the older style:
/// <https://dev-docs.kicad.org/en/file-formats/sexpr-pcb/>; the newer style is
/// known from a real file only (the format documents have not been checked).
///
/// Code 0 means "no net" in both. For a name-style file the codes below are
/// made up: the distinct names sorted, numbered from 1. They exist only inside
/// this tool and are never written; a file we write refers to nets by name.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetTable {
  /// References are by name (KiCad 10 style).
  pub by_name_style: bool,
  /// Name → made-up code; empty for a code-style file.
  pub codes: BTreeMap<String, u32>,
}

impl NetTable {
  /// The net code of an item with a `(net …)` child: a pad, track, via, arc or zone.
  pub fn code_of(&self, item: &List) -> Option<u32> {
    let n = item.child("net")?;
    if self.by_name_style {
      let name = n.atom(1)?;
      if name.is_empty() {
        return Some(0);
      }
      self.codes.get(name.as_ref()).copied()
    } else {
      n.number(1).map(|c| c as u32)
    }
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Board {
  pub footprints: Vec<Footprint>,
  /// Net code → name. KiCad 9 writes a top-level `(net …)` table; KiCad 10
  /// (file version 20260206 and later) has none and refers to nets by name, so
  /// the codes are then made up, see [`NetTable`].
  pub nets: BTreeMap<u32, String>,
  /// How nets are written in this file and how to read a reference.
  pub net_table: NetTable,
  /// Bounding box of every Edge.Cuts item; `None` if the board has no outline.
  pub outline: Option<BBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Footprint {
  /// Index of this footprint's node among the root list's items.
  pub node_index: usize,
  pub lib_id: String,
  pub reference: String,
  pub value: String,
  pub at: Point,
  /// Orientation in degrees, as written (counter-clockwise on screen).
  pub rotation: f64,
  pub side: Side,
  pub locked: bool,
  /// Tokens of `(attr …)`: `smd`, `through_hole`, `board_only`,
  /// `exclude_from_bom`, `exclude_from_pos_files`, `dnp`, …
  pub attrs: Vec<String>,
  pub pads: Vec<Pad>,
  /// `(clearance X)` written directly in the footprint: a local override of
  /// the clearance its pads demand (the USB-C receptacle J10 carries 0.13 mm).
  pub clearance: Option<f64>,
  /// Courtyard extent in the footprint's own frame, from graphics on the
  /// courtyard layer of its side. `None` if the footprint has none.
  pub courtyard_local: Option<BBox>,
  /// The courtyard outline itself, as edges in the footprint's own frame (arcs
  /// and circles sampled into chords). Concave outlines stay concave: an
  /// electrolytic can's courtyard is a cross, and parts legally sit in its
  /// notches. Empty if the footprint has none.
  pub courtyard_edges_local: Vec<Seg>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pad {
  /// Pad number as written; may be empty (mechanical pads) and is not unique.
  pub number: String,
  /// `smd`, `thru_hole`, `np_thru_hole` or `connect`.
  pub kind: String,
  /// Position in the footprint's frame.
  pub local: Point,
  /// Position on the board, see [`Footprint::to_board`].
  pub at: Point,
  /// The angle written in the pad's `(at …)`. KiCad stores pad orientation
  /// in board terms — footprint rotation already added — not relative to
  /// the footprint (measured, see `tests/corpus.rs`).
  pub angle: f64,
  pub size: Point,
  /// `circle`, `rect`, `roundrect`, `oval` or `custom`, as written (empty if absent).
  pub shape: String,
  /// `(roundrect_rratio r)` if the pad states one: corner radius = r x the shorter side.
  pub roundrect_rratio: Option<f64>,
  /// `gr_poly` primitives of a `custom` pad (empty for other shapes).
  pub primitives: Vec<PolyPrim>,
  /// `(options (anchor rect))` of a custom pad; the default anchor is a circle.
  pub anchor_rect: bool,
  /// Primitives of a `custom` pad that are not `gr_poly` (lines, circles,
  /// rectangles, arcs): present in the file, not modelled.
  pub unmodelled_primitives: usize,
  /// Local clearance override that applies to this pad: its own `(clearance X)`
  /// if it has one, else its footprint's. `None` means the net class rules.
  pub clearance: Option<f64>,
  /// Drill width and height; `None` for SMD pads. A round drill has both equal.
  pub drill: Option<Point>,
  pub net: Option<u32>,
  pub layers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
  NotABoard,
}

impl std::fmt::Display for BoardError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      BoardError::NotABoard => f.write_str("top-level list is not (kicad_pcb …)"),
    }
  }
}

impl std::error::Error for BoardError {}

impl Board {
  /// Extract the typed view. Unknown or malformed items are skipped, not
  /// fatal: a footprint with no `(at …)` is read at the origin, a pad with no
  /// number keeps an empty one. The tree itself is never changed.
  pub fn read(doc: &Document) -> Result<Board, BoardError> {
    let root = doc
      .root()
      .filter(|r| r.is("kicad_pcb"))
      .ok_or(BoardError::NotABoard)?;
    // A file with no top-level net table that refers to nets by name is
    // KiCad 10 style: collect the names (sorted, so the codes are the same on
    // every read of the same file).
    let has_table = root
      .children("net")
      .any(|n| n.number(1).is_some() && n.atom(2).is_some());
    let mut net_table = NetTable::default();
    if !has_table {
      let mut names = std::collections::BTreeSet::new();
      collect_net_names(root, &mut names);
      if !names.is_empty() {
        net_table.by_name_style = true;
        net_table.codes = names
          .into_iter()
          .enumerate()
          .map(|(i, n)| (n, i as u32 + 1))
          .collect();
      }
    }
    let mut board = Board {
      footprints: Vec::new(),
      nets: net_table.codes.iter().map(|(n, &c)| (c, n.clone())).collect(),
      net_table: net_table.clone(),
      outline: None,
    };
    for (i, item) in root.items.iter().enumerate() {
      let Some(list) = item.as_list() else { continue };
      let Some(head) = list.head() else { continue };
      match head.as_ref() {
        "footprint" => board.footprints.push(Footprint::read(i, list, &net_table)),
        "net" if !net_table.by_name_style => {
          if let (Some(code), Some(name)) = (list.number(1), list.atom(2)) {
            board.nets.insert(code as u32, name.into_owned());
          }
        }
        h if h.starts_with("gr_") && on_layer(list, "Edge.Cuts") => {
          board.outline = BBox::union(board.outline, graphic_extent(list));
        }
        _ => {}
      }
    }
    Ok(board)
  }

  pub fn footprint(&self, reference: &str) -> Option<&Footprint> {
    self.footprints.iter().find(|f| f.reference == reference)
  }

  /// Net code → (reference, pad index) for every pad on that net.
  pub fn pads_by_net(&self) -> HashMap<u32, Vec<(usize, usize)>> {
    let mut map: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
    for (fi, fp) in self.footprints.iter().enumerate() {
      for (pi, pad) in fp.pads.iter().enumerate() {
        if let Some(net) = pad.net {
          map.entry(net).or_default().push((fi, pi));
        }
      }
    }
    map
  }
}

impl Footprint {
  fn read(node_index: usize, list: &List, nets: &NetTable) -> Footprint {
    let (at, rotation) = read_at(list.child("at"));
    let side = match list.child("layer").and_then(|l| l.atom(1)).as_deref() {
      Some("B.Cu") => Side::Back,
      _ => Side::Front,
    };
    // KiCad 9 writes `(locked yes)`; older files write a bare `locked` atom
    // after the library id.
    let locked = list
      .child("locked")
      .is_some_and(|l| l.atom(1).as_deref() != Some("no"))
      || list
        .items
        .iter()
        .skip(2)
        .filter_map(|n| n.as_atom())
        .any(|a| a.value() == "locked");
    let mut fp = Footprint {
      node_index,
      lib_id: list.atom(1).unwrap_or_default().into_owned(),
      reference: property(list, "Reference").unwrap_or_default(),
      value: property(list, "Value").unwrap_or_default(),
      at,
      rotation,
      side,
      locked,
      attrs: list
        .child("attr")
        .map(|a| {
          a.items
            .iter()
            .skip(1)
            .filter_map(|n| n.as_atom())
            .map(|a| a.value().into_owned())
            .collect()
        })
        .unwrap_or_default(),
      pads: Vec::new(),
      clearance: list.child("clearance").and_then(|c| c.number(1)),
      courtyard_local: None,
      courtyard_edges_local: Vec::new(),
    };
    let courtyard = match side {
      Side::Front => "F.CrtYd",
      Side::Back => "B.CrtYd",
    };
    let mut edges = Vec::new();
    for child in list.items.iter().filter_map(|n| n.as_list()) {
      if child.is("pad") {
        let pad = fp.read_pad(child, nets);
        fp.pads.push(pad);
      } else if child.head().is_some_and(|h| h.starts_with("fp_")) && on_layer(child, courtyard) {
        edges.extend(graphic_edges(child));
      }
    }
    fp.courtyard_local = BBox::from_points(edges.iter().flat_map(|&(a, b)| [a, b]));
    fp.courtyard_edges_local = edges;
    fp
  }

  fn read_pad(&self, list: &List, nets: &NetTable) -> Pad {
    let (local, angle) = read_at(list.child("at"));
    let size = list
      .child("size")
      .map(|s| Point::new(s.number(1).unwrap_or(0.0), s.number(2).unwrap_or(0.0)))
      .unwrap_or_default();
    Pad {
      number: list.atom(1).unwrap_or_default().into_owned(),
      kind: list.atom(2).unwrap_or_default().into_owned(),
      local,
      at: self.to_board(local),
      angle,
      size,
      shape: list.atom(3).unwrap_or_default().into_owned(),
      roundrect_rratio: list.child("roundrect_rratio").and_then(|r| r.number(1)),
      primitives: custom_polys(list),
      anchor_rect: list
        .child("options")
        .and_then(|o| o.child("anchor"))
        .and_then(|a| a.atom(1))
        .is_some_and(|v| v == "rect"),
      unmodelled_primitives: list.child("primitives").map_or(0, |p| {
        p.items
          .iter()
          .filter_map(|n| n.as_list())
          .filter(|l| !l.is("gr_poly"))
          .count()
      }),
      clearance: list
        .child("clearance")
        .and_then(|c| c.number(1))
        .or(self.clearance),
      drill: list.child("drill").and_then(|d| {
        // `(drill 1.2)` or `(drill oval 1.0 2.0)`
        let at = |i| d.number(i);
        match (at(1), at(2)) {
          (Some(w), Some(h)) => Some(Point::new(w, h)),
          (Some(w), None) => Some(Point::new(w, w)),
          (None, _) => match (at(2), at(3)) {
            (Some(w), Some(h)) => Some(Point::new(w, h)),
            _ => None,
          },
        }
      }),
      net: nets.code_of(list),
      layers: list
        .child("layers")
        .map(|l| {
          l.items
            .iter()
            .skip(1)
            .filter_map(|n| n.as_atom())
            .map(|a| a.value().into_owned())
            .collect()
        })
        .unwrap_or_default(),
    }
  }

  /// Footprint frame → board frame. Rotation by the footprint's angle in
  /// KiCad's y-down screen convention, then translation. Back-side footprints
  /// need no extra mirror: KiCad stores their local coordinates already
  /// flipped. Both facts are checked against routed corpus boards, where every
  /// track ends on a pad centre (`tests/corpus.rs`).
  pub fn to_board(&self, local: Point) -> Point {
    let p = local.rotated(self.rotation) + self.at;
    // Resolved to whole nanometres, KiCad's internal unit. Summing decimal
    // millimetres lands a hair off an exact value (77.04595 became
    // 77.045949999…), which then rounds the other way from KiCad when printed:
    // 10 of 523 m1 pads disagreed in the fourth decimal before this.
    Point::new((p.x * 1e6).round() / 1e6, (p.y * 1e6).round() / 1e6)
  }
}

/// `(primitives (gr_poly (pts (xy …)…) (width w) (fill yes)))` of a custom pad.
fn custom_polys(pad: &List) -> Vec<PolyPrim> {
  let Some(prims) = pad.child("primitives") else {
    return Vec::new();
  };
  prims
    .children("gr_poly")
    .map(|g| PolyPrim {
      pts: g
        .child("pts")
        .map(|p| {
          p.children("xy")
            .filter_map(|l| Some(Point::new(l.number(1)?, l.number(2)?)))
            .collect()
        })
        .unwrap_or_default(),
      width: g.child("width").and_then(|w| w.number(1)).unwrap_or(0.0),
      fill: g
        .child("fill")
        .and_then(|f| f.atom(1))
        .is_none_or(|v| v != "no" && v != "none"),
    })
    .filter(|p| p.pts.len() >= 2)
    .collect()
}

/// Every distinct net name referred to as `(net "name")` anywhere under `list`.
fn collect_net_names(list: &List, out: &mut std::collections::BTreeSet<String>) {
  if list.is("net") {
    if let (Some(a), None) = (list.items.get(1).and_then(|n| n.as_atom()), list.items.get(2))
      && a.is_quoted()
      && !a.value().is_empty()
    {
      out.insert(a.value().into_owned());
    }
    return;
  }
  for item in &list.items {
    if let Some(l) = item.as_list() {
      collect_net_names(l, out);
    }
  }
}

fn read_at(at: Option<&List>) -> (Point, f64) {
  match at {
    Some(at) => (
      Point::new(at.number(1).unwrap_or(0.0), at.number(2).unwrap_or(0.0)),
      at.number(3).unwrap_or(0.0),
    ),
    None => (Point::default(), 0.0),
  }
}

fn property(list: &List, name: &str) -> Option<String> {
  list
    .children("property")
    .find(|p| p.atom(1).as_deref() == Some(name))
    .and_then(|p| p.atom(2))
    .map(|v| v.into_owned())
}

fn on_layer(list: &List, layer: &str) -> bool {
  list
    .child("layer")
    .and_then(|l| l.atom(1))
    .is_some_and(|l| l == layer)
}

/// Chords per arc / circle when sampling outlines. At 32 a 6.3 mm can's
/// courtyard is within 0.02 mm of the true circle.
const CURVE_SEGMENTS: usize = 32;

fn graphic_extent(list: &List) -> Option<BBox> {
  BBox::from_points(graphic_edges(list).into_iter().flat_map(|(a, b)| [a, b]))
}

/// The edges a `gr_*` / `fp_*` graphic draws: a line is one edge; a
/// rectangle, polygon or circle is a closed ring; an arc is an open chain
/// sampled along the curve (so its bulge is included, not just its three
/// defining points).
fn graphic_edges(list: &List) -> Vec<Seg> {
  let point = |k: &str| {
    list
      .child(k)
      .and_then(|p| Some(Point::new(p.number(1)?, p.number(2)?)))
  };
  let Some(head) = list.head() else {
    return Vec::new();
  };
  let kind = head.trim_start_matches("gr_").trim_start_matches("fp_");
  match kind {
    "line" => match (point("start"), point("end")) {
      (Some(a), Some(b)) => vec![(a, b)],
      _ => Vec::new(),
    },
    "rect" => match (point("start"), point("end")) {
      (Some(a), Some(b)) => closed(&[a, Point::new(b.x, a.y), b, Point::new(a.x, b.y)]),
      _ => Vec::new(),
    },
    "arc" => match (point("start"), point("mid"), point("end")) {
      (Some(s), Some(m), Some(e)) => chain(&geom::arc_points(s, m, e, CURVE_SEGMENTS)),
      _ => Vec::new(),
    },
    "circle" => match (point("center"), point("end")) {
      (Some(c), Some(e)) => closed(&geom::circle_points(c, (e - c).length(), CURVE_SEGMENTS)),
      _ => Vec::new(),
    },
    "poly" => list
      .child("pts")
      .map(|pts| {
        let v: Vec<Point> = pts
          .children("xy")
          .filter_map(|xy| Some(Point::new(xy.number(1)?, xy.number(2)?)))
          .collect();
        closed(&v)
      })
      .unwrap_or_default(),
    _ => Vec::new(),
  }
}

fn chain(pts: &[Point]) -> Vec<Seg> {
  pts
    .windows(2)
    .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
    .collect()
}

fn closed(pts: &[Point]) -> Vec<Seg> {
  let mut edges = chain(pts);
  if let (Some(&first), Some(&last)) = (pts.first(), pts.last())
    && pts.len() > 2
  {
    edges.push((last, first));
  }
  edges
}

#[cfg(test)]
mod net_style_tests {
  use super::*;
  use autolayout_sexpr::parse;

  const V9: &str = r#"(kicad_pcb (version 20241229)
  (net 0 "") (net 1 "GND") (net 2 "A")
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "R1")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 2 "A"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net 1 "GND")))
  (segment (start 0 0) (end 1 0) (width 0.2) (layer "F.Cu") (net 2))
)"#;

  const V10: &str = r#"(kicad_pcb (version 20260206)
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "R1")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net "A"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net "GND"))
    (pad "3" smd rect (at 4 0) (size 1 1) (layers "F.Cu")))
  (segment (start 0 0) (end 1 0) (width 0.2) (layer "F.Cu") (net "A"))
  (zone (net "GND") (layer "In1.Cu"))
)"#;

  #[test]
  fn code_style_file_keeps_its_codes() {
    let doc = parse(V9).unwrap();
    let b = Board::read(&doc).unwrap();
    assert!(!b.net_table.by_name_style);
    assert_eq!(b.nets.get(&2).map(String::as_str), Some("A"));
    assert_eq!(b.footprints[0].pads[0].net, Some(2));
    let seg = doc.root().unwrap().child("segment").unwrap();
    assert_eq!(b.net_table.code_of(seg), Some(2));
  }

  #[test]
  fn name_style_file_gets_sorted_made_up_codes() {
    let doc = parse(V10).unwrap();
    let b = Board::read(&doc).unwrap();
    assert!(b.net_table.by_name_style);
    // Sorted names, from 1: "A" is 1, "GND" is 2.
    assert_eq!(b.nets.get(&1).map(String::as_str), Some("A"));
    assert_eq!(b.nets.get(&2).map(String::as_str), Some("GND"));
    let pads = &b.footprints[0].pads;
    assert_eq!((pads[0].net, pads[1].net, pads[2].net), (Some(1), Some(2), None));
    let root = doc.root().unwrap();
    assert_eq!(b.net_table.code_of(root.child("segment").unwrap()), Some(1));
    assert_eq!(b.net_table.code_of(root.child("zone").unwrap()), Some(2));
    // Reading twice gives the same codes.
    assert_eq!(Board::read(&doc).unwrap(), b);
  }
}
