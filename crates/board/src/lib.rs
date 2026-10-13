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

/// Copper side a footprint is mounted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
  Front,
  Back,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Board {
  pub footprints: Vec<Footprint>,
  /// Net code → name, from the board's top-level `(net …)` table (KiCad 9).
  pub nets: BTreeMap<u32, String>,
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
    let mut board = Board {
      footprints: Vec::new(),
      nets: BTreeMap::new(),
      outline: None,
    };
    for (i, item) in root.items.iter().enumerate() {
      let Some(list) = item.as_list() else { continue };
      let Some(head) = list.head() else { continue };
      match head.as_ref() {
        "footprint" => board.footprints.push(Footprint::read(i, list)),
        "net" => {
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
  fn read(node_index: usize, list: &List) -> Footprint {
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
        let pad = fp.read_pad(child);
        fp.pads.push(pad);
      } else if child.head().is_some_and(|h| h.starts_with("fp_")) && on_layer(child, courtyard) {
        edges.extend(graphic_edges(child));
      }
    }
    fp.courtyard_local = BBox::from_points(edges.iter().flat_map(|&(a, b)| [a, b]));
    fp.courtyard_edges_local = edges;
    fp
  }

  fn read_pad(&self, list: &List) -> Pad {
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
      net: list.child("net").and_then(|n| n.number(1)).map(|c| c as u32),
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
