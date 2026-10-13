//! The routing model (M7.1): a grid per copper layer saying, for each cell
//! centre, which net may put copper there.
//!
//! One model is built for one *profile* — a net class's track width, via size
//! and clearance — because a centre-line may sit no nearer foreign copper than
//! `track/2 + clearance`, and a via centre no nearer than `via/2 + clearance`.
//! Pitch is `track + clearance`, so adjacent centre-lines on the grid are
//! legal. Obstacles are rasterised by the distance from each cell centre to
//! the real shape: rotated rectangles for rect/roundrect pads, capsules for
//! circle/oval pads, tracks and vias already on the board, and polygons for
//! keepouts. The board edge keeps `min_copper_edge_clearance`.
//!
//! Each cell holds an *owner*: free, one net, shared by several nets, or
//! blocked for all. A net may use a cell that is free or its own.
//!
//! What this does not model yet (each is reported or documented, none hidden):
//! the outline is its bounding box (a non-rectangular board is wrong at its
//! notches); `custom` pads use their anchor rectangle (the corpus has 32);
//! roundrect corners count as square (conservative); hole-to-hole between a
//! via and a pad drill uses the via-centre radius below; copper fills in
//! zones are not obstacles (counted in `ignored_fills`); footprint-level
//! keepouts are not read. Format reference for the tokens read:
//! <https://dev-docs.kicad.org/en/file-formats/sexpr-intro/>.

use std::collections::HashMap;
use std::fmt;

use autolayout_board::{BBox, Board, Point, arc_points};
use autolayout_sexpr::{Document, List};

use crate::rules::{DesignRules, NetClass};

/// Cell owner values. A net code `n` is stored as `n + 1`.
pub const FREE: u32 = 0;
/// Copper of a pad or track with no net: blocks every net.
pub const NO_NET: u32 = u32::MAX - 1;
/// More than one net claims the cell, or a hard obstacle sits on it.
pub const BLOCKED: u32 = u32::MAX;

/// Refuse grids bigger than this many cells (all layers): board files are
/// untrusted input (CLAUDE.md rule 5).
pub const MAX_CELLS: usize = 60_000_000;

#[derive(Debug, PartialEq)]
pub enum ModelError {
  NoOutline,
  TooLarge(usize),
  NotABoard,
  UnknownPlaneLayer(String),
  UnknownPlaneNet(String),
  NoCopperLayers,
}

impl fmt::Display for ModelError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      ModelError::NoOutline => write!(f, "board has no Edge.Cuts outline; cannot size a grid"),
      ModelError::TooLarge(n) => write!(f, "grid would have {n} cells (limit {MAX_CELLS}); refusing"),
      ModelError::NotABoard => write!(f, "not a kicad_pcb"),
      ModelError::UnknownPlaneLayer(l) => write!(f, "plane layer `{l}` is not a copper layer of this board"),
      ModelError::UnknownPlaneNet(n) => write!(f, "plane net `{n}` is not a net of this board"),
      ModelError::NoCopperLayers => write!(f, "board declares no copper layers"),
    }
  }
}

impl std::error::Error for ModelError {}

fn owner_of(net: Option<u32>) -> u32 {
  match net {
    Some(n) if n != 0 => n + 1,
    _ => NO_NET,
  }
}

/// Distance-to-shape primitives, all in board millimetres.
enum Shape {
  /// Oriented rectangle: centre, half extents, angle (degrees, board frame).
  Rect(Point, Point, f64),
  /// Segment with a round end of radius `r` (circles are zero-length).
  Capsule(Point, Point, f64),
  Polygon(Vec<Point>),
}

impl Shape {
  /// Distance from `p` to the shape's edge, 0 inside.
  fn distance(&self, p: Point) -> f64 {
    match self {
      Shape::Rect(c, half, angle) => {
        let l = (p - *c).rotated(-angle);
        (l.x.abs() - half.x).max(0.0).hypot((l.y.abs() - half.y).max(0.0))
      }
      Shape::Capsule(a, b, r) => (seg_dist(p, *a, *b) - r).max(0.0),
      Shape::Polygon(pts) => {
        if inside(pts, p) {
          0.0
        } else {
          let mut d = f64::INFINITY;
          for i in 0..pts.len() {
            if let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % pts.len())) {
              d = d.min(seg_dist(p, a, b));
            }
          }
          d
        }
      }
    }
  }

  fn bbox(&self) -> Option<BBox> {
    match self {
      Shape::Rect(c, half, _) => {
        let r = half.x.hypot(half.y);
        Some(BBox {
          min: Point::new(c.x - r, c.y - r),
          max: Point::new(c.x + r, c.y + r),
        })
      }
      Shape::Capsule(a, b, r) => Some(BBox {
        min: Point::new(a.x.min(b.x) - r, a.y.min(b.y) - r),
        max: Point::new(a.x.max(b.x) + r, a.y.max(b.y) + r),
      }),
      Shape::Polygon(pts) => BBox::from_points(pts.iter().copied()),
    }
  }
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
  let ab = b - a;
  let l2 = ab.x * ab.x + ab.y * ab.y;
  let t = if l2 == 0.0 {
    0.0
  } else {
    (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / l2).clamp(0.0, 1.0)
  };
  (p - Point::new(a.x + ab.x * t, a.y + ab.y * t)).length()
}

fn inside(pts: &[Point], p: Point) -> bool {
  let mut c = false;
  let mut j = pts.len().saturating_sub(1);
  for i in 0..pts.len() {
    if let (Some(a), Some(b)) = (pts.get(i), pts.get(j))
      && (a.y > p.y) != (b.y > p.y)
      && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
    {
      c = !c;
    }
    j = i;
  }
  c
}

/// A plane: a copper layer poured with one net, not routed on.
#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
  pub layer: String,
  pub net: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelStats {
  pub existing_segments: usize,
  pub existing_vias: usize,
  pub keepout_zones: usize,
  /// Zones with a copper fill that are not obstacles in this model.
  pub ignored_fills: usize,
  pub custom_pads: usize,
}

pub struct RoutingModel {
  pub origin: Point,
  pub pitch: f64,
  pub nx: usize,
  pub ny: usize,
  /// Routable copper layers, in stack order. Plane layers are not here.
  pub layers: Vec<String>,
  /// Every copper layer the file declares, outermost first (a through-via
  /// spans the first and last).
  pub stack: Vec<String>,
  pub planes: Vec<Plane>,
  /// Per layer, `ny * nx` owners: where a centre-line of this profile may go.
  pub track: Vec<Vec<u32>>,
  /// One map for vias (they pierce every layer): where a via centre may go.
  pub via: Vec<u32>,
  pub profile: NetClass,
  pub stats: ModelStats,
}

/// Result of asking whether a pad can be reached on the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Access {
  /// (layer index, ix, iy) of cells inside the pad's copper the net may use.
  pub cells: Vec<(usize, usize, usize)>,
}

impl RoutingModel {
  pub fn index(&self, ix: usize, iy: usize) -> usize {
    iy * self.nx + ix
  }

  pub fn centre(&self, ix: usize, iy: usize) -> Point {
    Point::new(
      self.origin.x + ix as f64 * self.pitch,
      self.origin.y + iy as f64 * self.pitch,
    )
  }

  /// May `net` put a centre-line on this cell of routable layer `layer`?
  pub fn track_ok(&self, layer: usize, ix: usize, iy: usize, net: u32) -> bool {
    let o = self
      .track
      .get(layer)
      .and_then(|m| m.get(self.index(ix, iy)))
      .copied()
      .unwrap_or(BLOCKED);
    o == FREE || o == net + 1
  }

  pub fn via_ok(&self, ix: usize, iy: usize, net: u32) -> bool {
    let o = self.via.get(self.index(ix, iy)).copied().unwrap_or(BLOCKED);
    o == FREE || o == net + 1
  }

  /// Layer indices (into `self.layers`) a pad's copper is on.
  pub fn pad_layer_indices(&self, names: &[String]) -> Vec<usize> {
    let mut out = Vec::new();
    for n in names {
      for (i, l) in self.layers.iter().enumerate() {
        let hit = match n.as_str() {
          "*.Cu" => true,
          "F&B.Cu" => l == "F.Cu" || l == "B.Cu",
          s => s == l,
        };
        if hit && !out.contains(&i) {
          out.push(i);
        }
      }
    }
    out.sort_unstable();
    out
  }

  /// Cells inside pad `pad` of `board` where its net may place a centre-line.
  pub fn pad_access(&self, board: &Board, fi: usize, pi: usize) -> Access {
    let mut cells = Vec::new();
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return Access { cells };
    };
    let Some(net) = pad.net else {
      return Access { cells };
    };
    let shape = pad_shape(pad);
    let Some(bb) = shape.bbox() else {
      return Access { cells };
    };
    let (x0, x1, y0, y1) = self.cell_range(bb, 0.0);
    for layer in self.pad_layer_indices(&pad.layers) {
      for iy in y0..=y1 {
        for ix in x0..=x1 {
          if shape.distance(self.centre(ix, iy)) == 0.0 && self.track_ok(layer, ix, iy, net) {
            cells.push((layer, ix, iy));
          }
        }
      }
    }
    Access { cells }
  }

  /// Inclusive cell index range covering `bb` grown by `grow`, clamped to the grid.
  fn cell_range(&self, bb: BBox, grow: f64) -> (usize, usize, usize, usize) {
    let lo = |v: f64, o: f64, n: usize| {
      (((v - grow - o) / self.pitch).floor().max(0.0) as usize).min(n.saturating_sub(1))
    };
    let hi = |v: f64, o: f64, n: usize| {
      (((v + grow - o) / self.pitch).ceil().max(0.0) as usize).min(n.saturating_sub(1))
    };
    (
      lo(bb.min.x, self.origin.x, self.nx),
      hi(bb.max.x, self.origin.x, self.nx),
      lo(bb.min.y, self.origin.y, self.ny),
      hi(bb.max.y, self.origin.y, self.ny),
    )
  }

  /// Claim every cell whose centre is within `radius` of `shape`.
  fn paint(&self, map: &mut [u32], shape: &Shape, radius: f64, owner: u32) {
    let Some(bb) = shape.bbox() else { return };
    let (x0, x1, y0, y1) = self.cell_range(bb, radius);
    for iy in y0..=y1 {
      for ix in x0..=x1 {
        if shape.distance(self.centre(ix, iy)) <= radius
          && let Some(c) = map.get_mut(iy * self.nx + ix)
        {
          *c = claim(*c, owner);
        }
      }
    }
  }
}

fn claim(cell: u32, owner: u32) -> u32 {
  if cell == FREE || cell == owner {
    owner
  } else {
    BLOCKED
  }
}

fn pad_shape(pad: &autolayout_board::Pad) -> Shape {
  let half = Point::new(pad.size.x / 2.0, pad.size.y / 2.0);
  match pad.shape.as_str() {
    "circle" | "oval" => {
      // A capsule along the long axis; a circle has equal sides.
      let r = half.x.min(half.y);
      let along = Point::new((half.x - r).max(0.0), (half.y - r).max(0.0));
      let a = pad.at + along.rotated(pad.angle);
      let b = pad.at - along.rotated(pad.angle);
      Shape::Capsule(a, b, r)
    }
    _ => Shape::Rect(pad.at, half, pad.angle),
  }
}

/// Copper layers declared by the file, in stack order.
fn copper_layers(root: &List) -> Vec<String> {
  root
    .child("layers")
    .map(|l| {
      l.items
        .iter()
        .filter_map(|n| n.as_list())
        .filter_map(|e| e.atom(1))
        .filter(|n| n.ends_with(".Cu"))
        .map(|n| n.into_owned())
        .collect()
    })
    .unwrap_or_default()
}

fn xy(l: Option<&List>) -> Option<Point> {
  let l = l?;
  Some(Point::new(l.number(1)?, l.number(2)?))
}

impl RoutingModel {
  /// Build the model for `profile` (a class in `rules`). `planes` are
  /// `(layer name, net name)` pairs; the caller says which layers are poured
  /// because a stripped board no longer says.
  pub fn build(
    doc: &Document,
    board: &Board,
    rules: &DesignRules,
    profile: &NetClass,
    planes: &[(String, String)],
  ) -> Result<RoutingModel, ModelError> {
    let root = doc
      .root()
      .filter(|r| r.is("kicad_pcb"))
      .ok_or(ModelError::NotABoard)?;
    let outline = board.outline.ok_or(ModelError::NoOutline)?;
    let all_cu = copper_layers(root);
    if all_cu.is_empty() {
      return Err(ModelError::NoCopperLayers);
    }
    let net_by_name: HashMap<&str, u32> = board.nets.iter().map(|(&c, n)| (n.as_str(), c)).collect();
    let mut plane_list = Vec::new();
    for (layer, net) in planes {
      if !all_cu.contains(layer) {
        return Err(ModelError::UnknownPlaneLayer(layer.clone()));
      }
      let code = *net_by_name
        .get(net.as_str())
        .ok_or_else(|| ModelError::UnknownPlaneNet(net.clone()))?;
      plane_list.push(Plane {
        layer: layer.clone(),
        net: code,
      });
    }
    let layers: Vec<String> = all_cu
      .iter()
      .filter(|l| !plane_list.iter().any(|p| &p.layer == *l))
      .cloned()
      .collect();

    let base_cl = profile.clearance.max(rules.min.clearance);
    let pitch = profile.track_width + base_cl;
    let nx = ((outline.width() / pitch).floor() as usize).saturating_add(1);
    let ny = ((outline.height() / pitch).floor() as usize).saturating_add(1);
    let cells = nx.saturating_mul(ny).saturating_mul(layers.len().max(1));
    if !pitch.is_finite() || pitch <= 0.0 || cells > MAX_CELLS {
      return Err(ModelError::TooLarge(cells));
    }

    let mut m = RoutingModel {
      origin: outline.min,
      pitch,
      nx,
      ny,
      track: vec![vec![FREE; nx * ny]; layers.len()],
      via: vec![FREE; nx * ny],
      layers,
      stack: all_cu.clone(),
      planes: plane_list,
      profile: profile.clone(),
      stats: ModelStats::default(),
    };

    let name = |code: Option<u32>| {
      code
        .and_then(|c| board.nets.get(&c))
        .map(String::as_str)
        .unwrap_or("")
    };
    let cl_for = |owner_net: Option<u32>| base_cl.max(rules.class_for_net(name(owner_net)).clearance);
    let (tr, vr) = (profile.track_width / 2.0, profile.via_diameter / 2.0);
    let (mut track_maps, mut via_map) = (std::mem::take(&mut m.track), std::mem::take(&mut m.via));

    // Pads.
    for fp in &board.footprints {
      for pad in &fp.pads {
        let shape = pad_shape(pad);
        if pad.shape == "custom" {
          m.stats.custom_pads += 1;
        }
        let owner = owner_of(pad.net);
        let cl = cl_for(pad.net);
        // np_thru_hole pads carry no copper on the layers they list; the hole
        // itself is an obstacle below.
        let has_copper = pad.kind != "np_thru_hole";
        if has_copper {
          for layer in m.pad_layer_indices(&pad.layers) {
            if let Some(map) = track_maps.get_mut(layer) {
              m.paint(map, &shape, tr + cl, owner);
            }
            m.paint(&mut via_map, &shape, vr + cl, owner);
          }
        }
        if let Some(d) = pad.drill {
          let hole = Shape::Capsule(pad.at, pad.at, d.x.max(d.y) / 2.0);
          let hc = rules.min.hole_clearance;
          if !has_copper {
            for map in &mut track_maps {
              m.paint(map, &hole, tr + hc, BLOCKED);
            }
            m.paint(&mut via_map, &hole, vr + hc, BLOCKED);
          }
          // A via drill may not sit nearer a pad drill than hole_to_hole.
          m.paint(
            &mut via_map,
            &hole,
            profile.via_drill / 2.0 + rules.min.hole_to_hole,
            BLOCKED,
          );
        }
      }
    }

    // Copper already on the board: tracks and vias keep their nets' space.
    for item in root.items.iter().filter_map(|n| n.as_list()) {
      match item.head().as_deref() {
        Some("segment") | Some("arc") => {
          let (Some(layer), Some(w)) = (
            item.child("layer").and_then(|l| l.atom(1)),
            item.child("width").and_then(|w| w.number(1)),
          ) else {
            continue;
          };
          let Some(li) = m.layers.iter().position(|l| *l == layer) else {
            continue;
          };
          let net = item.child("net").and_then(|n| n.number(1)).map(|n| n as u32);
          let pts: Vec<Point> = if item.is("arc") {
            match (
              xy(item.child("start")),
              xy(item.child("mid")),
              xy(item.child("end")),
            ) {
              (Some(a), Some(b), Some(c)) => arc_points(a, b, c, 16),
              _ => continue,
            }
          } else {
            match (xy(item.child("start")), xy(item.child("end"))) {
              (Some(a), Some(b)) => vec![a, b],
              _ => continue,
            }
          };
          m.stats.existing_segments += 1;
          let (owner, cl) = (owner_of(net), cl_for(net));
          for pair in pts.windows(2) {
            if let [a, b] = pair {
              let shape = Shape::Capsule(*a, *b, w / 2.0);
              if let Some(map) = track_maps.get_mut(li) {
                m.paint(map, &shape, tr + cl, owner);
              }
              m.paint(&mut via_map, &shape, vr + cl, owner);
            }
          }
        }
        Some("via") => {
          let (Some(at), Some(size)) = (xy(item.child("at")), item.child("size").and_then(|s| s.number(1)))
          else {
            continue;
          };
          let net = item.child("net").and_then(|n| n.number(1)).map(|n| n as u32);
          m.stats.existing_vias += 1;
          let shape = Shape::Capsule(at, at, size / 2.0);
          let (owner, cl) = (owner_of(net), cl_for(net));
          for map in &mut track_maps {
            m.paint(map, &shape, tr + cl, owner);
          }
          m.paint(&mut via_map, &shape, vr + cl, owner);
        }
        Some("zone") => {
          let ko = item.child("keepout");
          let Some(poly) = item
            .child("polygon")
            .and_then(|p| p.child("pts"))
            .map(|p| p.children("xy").filter_map(|l| xy(Some(l))).collect::<Vec<_>>())
          else {
            continue;
          };
          let Some(ko) = ko else {
            if item.child("fill").is_some()
              && item
                .child("net")
                .and_then(|n| n.number(1))
                .is_some_and(|n| n != 0.0)
            {
              m.stats.ignored_fills += 1;
            }
            continue;
          };
          let denied = |what: &str| ko.child(what).and_then(|c| c.atom(1)).as_deref() == Some("not_allowed");
          let zone_layers: Vec<String> = match (item.child("layers"), item.child("layer")) {
            (Some(l), _) => l
              .items
              .iter()
              .skip(1)
              .filter_map(|n| n.as_atom())
              .map(|a| a.value().into_owned())
              .collect(),
            (None, Some(l)) => l.atom(1).map(|a| vec![a.into_owned()]).unwrap_or_default(),
            _ => Vec::new(),
          };
          let shape = Shape::Polygon(poly);
          let hits = |l: &str| {
            zone_layers
              .iter()
              .any(|z| z == l || z == "*.Cu" || (z == "F&B.Cu" && (l == "F.Cu" || l == "B.Cu")))
          };
          let mut used = false;
          if denied("tracks") {
            for (li, name) in m.layers.iter().enumerate() {
              if hits(name)
                && let Some(map) = track_maps.get_mut(li)
              {
                m.paint(map, &shape, tr, BLOCKED);
                used = true;
              }
            }
          }
          if denied("vias") && m.layers.iter().any(|l| hits(l)) {
            m.paint(&mut via_map, &shape, vr, BLOCKED);
            used = true;
          }
          if used {
            m.stats.keepout_zones += 1;
          }
        }
        _ => {}
      }
    }

    // Board edge: bounding box of Edge.Cuts, with min_copper_edge_clearance.
    let edge = rules.min.copper_edge_clearance;
    for (map, r) in track_maps
      .iter_mut()
      .map(|t| (t, tr))
      .chain(std::iter::once((&mut via_map, vr)))
    {
      let keep = edge + r;
      for iy in 0..ny {
        for ix in 0..nx {
          let p = m.centre(ix, iy);
          let near_edge = p.x - outline.min.x < keep
            || outline.max.x - p.x < keep
            || p.y - outline.min.y < keep
            || outline.max.y - p.y < keep;
          if near_edge && let Some(c) = map.get_mut(iy * nx + ix) {
            *c = BLOCKED;
          }
        }
      }
    }

    m.track = track_maps;
    m.via = via_map;
    Ok(m)
  }

  /// Fraction of cells of each routable layer a given net cannot use.
  pub fn blocked_fraction(&self, layer: usize) -> f64 {
    let Some(map) = self.track.get(layer) else {
      return 0.0;
    };
    map.iter().filter(|&&o| o != FREE).count() as f64 / map.len().max(1) as f64
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use autolayout_sexpr::parse;

  // 20 x 10 mm board, 2 signal layers + one inner plane layer. U1 pad 1 is a
  // 2 x 1 rect rotated 90° (so 1 wide, 2 tall), net 1; pad 2 a through-hole
  // circle, net 2; an np_thru_hole; a keepout that forbids tracks on F.Cu.
  const BOARD: &str = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (4 "In1.Cu" power) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "G")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 5 5) (property "Reference" "U1")
    (pad "1" smd rect (at 0 0 90) (size 2 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" thru_hole circle (at 6 0) (size 1.6 1.6) (drill 0.8) (layers "*.Cu") (net 2 "B"))
    (pad "" np_thru_hole circle (at 6 3) (size 1 1) (drill 1) (layers "*.Cu")))
  (zone (net 0) (net_name "") (layer "F.Cu")
    (keepout (tracks not_allowed) (vias allowed) (pads allowed) (copperpour allowed) (footprints allowed))
    (polygon (pts (xy 14 2) (xy 18 2) (xy 18 4) (xy 14 4)))))"#;

  fn rules() -> DesignRules {
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.5;
    r.min.hole_clearance = 0.2;
    r.min.hole_to_hole = 0.3;
    r
  }

  fn build(planes: &[(String, String)]) -> (Board, RoutingModel) {
    let doc = parse(BOARD).unwrap();
    let board = Board::read(&doc).unwrap();
    let r = rules();
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &r, &p, planes).unwrap();
    (board, m)
  }

  fn cell_at(m: &RoutingModel, x: f64, y: f64) -> (usize, usize) {
    (
      ((x - m.origin.x) / m.pitch).round() as usize,
      ((y - m.origin.y) / m.pitch).round() as usize,
    )
  }

  #[test]
  fn pitch_is_track_plus_clearance() {
    let (_, m) = build(&[]);
    assert!((m.pitch - 0.4).abs() < 1e-12);
    assert_eq!(m.layers, vec!["F.Cu", "In1.Cu", "B.Cu"]);
  }

  #[test]
  fn plane_layer_is_not_routable() {
    let (_, m) = build(&[("In1.Cu".into(), "G".into())]);
    assert_eq!(m.layers, vec!["F.Cu", "B.Cu"]);
    assert_eq!(
      m.planes,
      vec![Plane {
        layer: "In1.Cu".into(),
        net: 3
      }]
    );
  }

  #[test]
  fn bad_plane_specs_are_errors() {
    let doc = parse(BOARD).unwrap();
    let board = Board::read(&doc).unwrap();
    let r = rules();
    let p = r.classes[0].clone();
    let bad = |l: &str, n: &str| RoutingModel::build(&doc, &board, &r, &p, &[(l.into(), n.into())]).err();
    assert_eq!(
      bad("X.Cu", "G"),
      Some(ModelError::UnknownPlaneLayer("X.Cu".into()))
    );
    assert_eq!(
      bad("In1.Cu", "nope"),
      Some(ModelError::UnknownPlaneNet("nope".into()))
    );
  }

  #[test]
  fn rotated_rect_pad_is_tall_not_wide() {
    // Pad 1 at (5,5), 2x1 rotated 90°: 1 wide (x), 2 tall (y). A foreign
    // centre-line needs track/2 + clearance = 0.3 beyond the copper.
    let (_, m) = build(&[]);
    let foreign = 2; // net "B"
    let tall_y = cell_at(&m, 5.0, 6.4); // 0.4 clear of the short end
    assert!(m.track_ok(0, tall_y.0, tall_y.1, foreign));
    // On the end of a tall pad (y = 6.0) the foreign cell is blocked; beside
    // it (x = 6.0, 0.5 from the long edge) it is free. A pad that were wide
    // instead would reverse both.
    let c = cell_at(&m, 5.0, 6.0);
    assert!(!m.track_ok(0, c.0, c.1, foreign), "pad should extend 1 mm in y");
    let c = cell_at(&m, 6.0, 5.0);
    assert!(
      m.track_ok(0, c.0, c.1, foreign),
      "pad should be only 1 mm wide in x"
    );
    // Its own net may sit on it; the pad is SMD on F.Cu only, so B.Cu is free.
    let c = cell_at(&m, 5.0, 5.0);
    assert!(m.track_ok(0, c.0, c.1, 1));
    assert!(m.track_ok(1, c.0, c.1, foreign));
  }

  #[test]
  fn through_hole_pad_blocks_every_layer_and_the_via_map() {
    let (_, m) = build(&[]);
    let c = cell_at(&m, 11.0, 5.0);
    assert!(m.track_ok(0, c.0, c.1, 2) && m.track_ok(1, c.0, c.1, 2));
    assert!(!m.track_ok(0, c.0, c.1, 1) && !m.track_ok(1, c.0, c.1, 1));
    // A via may not sit on a drilled hole, even its own net's pad.
    assert!(!m.via_ok(c.0, c.1, 1) && !m.via_ok(c.0, c.1, 2));
  }

  #[test]
  fn np_hole_blocks_everyone() {
    let (_, m) = build(&[]);
    let c = cell_at(&m, 11.0, 8.0);
    assert!(!m.track_ok(0, c.0, c.1, 1) && !m.track_ok(1, c.0, c.1, 2));
    assert!(!m.via_ok(c.0, c.1, 1));
  }

  #[test]
  fn edge_and_keepout_block() {
    let (_, m) = build(&[]);
    // Edge: clearance 0.5 + track/2 0.1 = 0.6 from the outline.
    assert!(!m.track_ok(0, 0, 3, 1) && !m.track_ok(0, 1, 3, 1));
    let c = cell_at(&m, 3.0, 1.2);
    assert!(m.track_ok(0, c.0, c.1, 1));
    // Keepout polygon forbids tracks on F.Cu only, vias still allowed.
    let k = cell_at(&m, 16.0, 3.0);
    assert!(!m.track_ok(0, k.0, k.1, 1));
    assert!(m.track_ok(1, k.0, k.1, 1));
    assert!(m.via_ok(k.0, k.1, 1));
    assert_eq!(m.stats.keepout_zones, 1);
  }

  #[test]
  fn pad_access_finds_cells_inside_and_none_off_grid_small() {
    let (b, m) = build(&[]);
    let a = m.pad_access(&b, 0, 0);
    assert!(!a.cells.is_empty());
    for &(l, ix, iy) in &a.cells {
      assert_eq!(l, 0, "SMD pad on F.Cu only");
      assert!(m.centre(ix, iy).x >= 4.5 - 1e-9 && m.centre(ix, iy).x <= 5.5 + 1e-9);
    }
    let t = m.pad_access(&b, 0, 1);
    assert!(t.cells.iter().any(|c| c.0 == 0) && t.cells.iter().any(|c| c.0 == 1));
  }

  #[test]
  fn refuses_what_it_cannot_size() {
    let doc = parse("(kicad_pcb (layers (0 \"F.Cu\" signal)))").unwrap();
    let board = Board::read(&doc).unwrap();
    let r = rules();
    let p = r.classes[0].clone();
    assert_eq!(
      RoutingModel::build(&doc, &board, &r, &p, &[]).err(),
      Some(ModelError::NoOutline)
    );
  }

  #[test]
  fn arbitrary_text_never_panics() {
    for t in ["(kicad_pcb)", "(kicad_pcb (layers))", "(x)"] {
      if let Ok(doc) = parse(t)
        && let Ok(board) = Board::read(&doc)
      {
        let r = rules();
        let p = r.classes[0].clone();
        let _ = RoutingModel::build(&doc, &board, &r, &p, &[]);
      }
    }
  }
}
