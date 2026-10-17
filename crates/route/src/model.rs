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

use std::collections::{HashMap, HashSet};
use std::fmt;

use autolayout_board::{BBox, Board, Point, arc_points};
use autolayout_sexpr::{Document, List};

use crate::rules::{DesignRules, NetClass};
use crate::shape::{Shape, pad_copper, pad_extent, pad_obstacle};

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

/// Grid cells (layer, ix, iy) a search for a net may start from.
pub type SeedCells = Vec<(usize, usize, usize)>;

/// The grid line a pad's stub should land on: a column (`ix`) for a pad long
/// in y, a row (`iy`) for a pad long in x.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lane {
  pub column: bool,
  pub index: usize,
}

/// Lanes tried either side of a pad's nearest grid line.
const LANE_SPAN: i64 = 3;

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
  /// Least centre-to-centre distance between two via holes: drill plus
  /// `min_hole_to_hole`. Two vias one pitch apart broke this on m1 (KiCad
  /// hole_to_hole, 2026-10-13).
  pub via_spacing: f64,
  /// Extra clearance kept on top of every rule: the project's `max_error`.
  pub margin: f64,
  pub stats: ModelStats,
  /// Pads (footprint index, pad index) that already have copper of their own
  /// net attached in the file (a track end or a via inside the pad), with the
  /// grid cells (layer, ix, iy) where that copper passes exactly through a
  /// grid node: a search for the net may start from those cells. Only pads
  /// with at least one such cell are listed. See [`RoutingModel::find_preconnected`].
  pub preconnected: HashMap<(usize, usize), SeedCells>,
  /// Pads whose attached same-net copper includes a via (a through via reaches
  /// every plane): a plane-net pad listed here needs no tap.
  pub preconnected_via: HashSet<(usize, usize)>,
  /// Planned grid lane per pad of a row of long pads (see `plan_lanes`).
  pub lanes: HashMap<(usize, usize), Lane>,
  /// Centres of the `preconnected` seed cells with their net: the ways out of
  /// planned escapes. A plane tap via placed near one would close it.
  pub seed_points: Vec<(Point, u32)>,
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

  /// Forbid a via at the grid cell nearest `p`, for every net. Used to move a
  /// plane tap off a site where the emulated pour left it cut off from its
  /// plane (see `pocket`). Returns whether a free site was blocked (false:
  /// off the grid, or already blocked).
  pub fn block_via_near(&mut self, p: Point) -> bool {
    let fx = (p.x - self.origin.x) / self.pitch;
    let fy = (p.y - self.origin.y) / self.pitch;
    if !(fx.is_finite() && fy.is_finite()) || fx < -0.5 || fy < -0.5 {
      return false;
    }
    let (ix, iy) = (fx.round() as usize, fy.round() as usize);
    if ix >= self.nx || iy >= self.ny {
      return false;
    }
    let i = self.index(ix, iy);
    match self.via.get_mut(i) {
      Some(c) if *c != BLOCKED => {
        *c = BLOCKED;
        true
      }
      _ => false,
    }
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
    let shape = pad_copper(pad);
    // The track end must sit fully on copper: inset by half the track width.
    let inset = self.profile.track_width / 2.0;
    let Some(bb) = shape.bbox() else {
      return Access { cells };
    };
    let (x0, x1, y0, y1) = self.cell_range(bb, 0.0);
    for layer in self.pad_layer_indices(&pad.layers) {
      for iy in y0..=y1 {
        for ix in x0..=x1 {
          if shape.sdf(self.centre(ix, iy)) <= -inset && self.track_ok(layer, ix, iy, net) {
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

    // Every net is routed under this one profile, so the spacing must satisfy
    // the largest class clearance in the project: two nets are kept apart by
    // the larger of their classes' clearances, and a net of a wider-clearance
    // class (g2i's `+12V`, 0.25 mm against Default's 0.2) would otherwise sit
    // too near its neighbours (18 KiCad-style findings on that board, round 8
    // dry run). Net-class-specific widths and spacing are not routed yet.
    let margin = rules.min.max_error;
    let base_cl = rules
      .classes
      .iter()
      .map(|c| c.clearance)
      .fold(profile.clearance.max(rules.min.clearance), f64::max)
      + margin;
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
      via_spacing: profile.via_drill + rules.min.hole_to_hole,
      margin: rules.min.max_error,
      stats: ModelStats::default(),
      preconnected: HashMap::new(),
      preconnected_via: HashSet::new(),
      lanes: HashMap::new(),
      seed_points: Vec::new(),
    };

    let name = |code: Option<u32>| {
      code
        .and_then(|c| board.nets.get(&c))
        .map(String::as_str)
        .unwrap_or("")
    };
    let cl_for =
      |owner_net: Option<u32>| base_cl.max(rules.class_for_net(name(owner_net)).clearance + margin);
    let (tr, vr) = (profile.track_width / 2.0, profile.via_diameter / 2.0);
    let (mut track_maps, mut via_map) = (std::mem::take(&mut m.track), std::mem::take(&mut m.via));

    // Pads.
    for fp in &board.footprints {
      for pad in &fp.pads {
        let shape = pad_obstacle(pad);
        if pad.shape == "custom" {
          m.stats.custom_pads += 1;
        }
        let owner = owner_of(pad.net);
        // A local override replaces the class clearance for this pad's copper
        // (KiCad takes the larger of the two items' clearances for a pair).
        let cl = match pad.clearance {
          Some(o) => base_cl.max(o + margin),
          None => cl_for(pad.net),
        };
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
          let net = board.net_table.code_of(item);
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
          let net = board.net_table.code_of(item);
          m.stats.existing_vias += 1;
          let shape = Shape::Capsule(at, at, size / 2.0);
          let (owner, cl) = (owner_of(net), cl_for(net));
          for map in &mut track_maps {
            m.paint(map, &shape, tr + cl, owner);
          }
          m.paint(&mut via_map, &shape, vr + cl, owner);
          let hole = Shape::Capsule(at, at, 0.0);
          m.paint(&mut via_map, &hole, m.via_spacing - 1e-9, BLOCKED);
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
            if item.child("fill").is_some() && board.net_table.code_of(item).is_some_and(|n| n != 0) {
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
    m.find_preconnected(board, root);
    m.plan_lanes(board);
    Ok(m)
  }

  /// Find the pads that already have copper of their own net attached in the
  /// file, and where that copper can be joined on the grid.
  ///
  /// A pad has copper attached when a track end lies inside its copper on one
  /// of its layers, or a via sits inside it. The copper is followed through
  /// tracks that share an end (same layer, within 1 um) and vias at a track
  /// end (within 10 um). Grid nodes that lie on that copper (within 20 um of a
  /// track's centre-line, or of a via's centre on every layer) and that the
  /// net may use become `preconnected` cells: the router starts from them
  /// without drawing any stub, because the copper already runs through them.
  /// Joint choice of a grid lane for every pad in a row of long pads (a QFN or
  /// SOIC edge). Pads 0.4 mm apart on a 0.31 mm grid would each take the nearest
  /// line and share it with a neighbour; instead the pads of a row, in order
  /// along the row, get strictly increasing lines of least total offset (dynamic
  /// programming, as the pin-access literature does per cell). Rows are pads of
  /// one part with the same long axis, on the same side and the same across-axis
  /// position. A row with no feasible assignment gets none.
  fn plan_lanes(&mut self, board: &Board) {
    for (fi, fp) in board.footprints.iter().enumerate() {
      let count = fp.pads.len().max(1) as f64;
      let (mx, my) = fp
        .pads
        .iter()
        .fold((0.0, 0.0), |(a, b), q| (a + q.at.x, b + q.at.y));
      let mut rows: std::collections::BTreeMap<(bool, i64, i64), Vec<(f64, usize)>> =
        std::collections::BTreeMap::new();
      for (pi, pad) in fp.pads.iter().enumerate() {
        if pad.net.is_none() {
          continue;
        }
        let (w, h) = pad_extent(pad);
        let vertical = if h >= 1.5 * w {
          true
        } else if w >= 1.5 * h {
          false
        } else {
          continue;
        };
        // Lateral position runs across the pad's short side; `across` along its long side.
        let (lat, across, mid) = if vertical {
          (pad.at.x, pad.at.y, my / count)
        } else {
          (pad.at.y, pad.at.x, mx / count)
        };
        let side = if across >= mid { 1 } else { -1 };
        rows
          .entry((vertical, side, (across / 0.05).round() as i64))
          .or_default()
          .push((lat, pi));
      }
      for ((vertical, _, _), mut row) in rows {
        if row.len() < 2 {
          continue;
        }
        row.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let origin = if vertical { self.origin.x } else { self.origin.y };
        let line = |j: i64| origin + j as f64 * self.pitch;
        let cands: Vec<Vec<i64>> = row
          .iter()
          .map(|&(t, _)| {
            let c = ((t - origin) / self.pitch).round() as i64;
            (c - LANE_SPAN..=c + LANE_SPAN).filter(|&j| j >= 0).collect()
          })
          .collect();
        // best[i][k]: least total offset with pad i on cands[i][k]; from[i][k] its predecessor.
        let mut best: Vec<Vec<f64>> = Vec::new();
        let mut from: Vec<Vec<usize>> = Vec::new();
        for (i, cs) in cands.iter().enumerate() {
          let mut b = vec![f64::INFINITY; cs.len()];
          let mut f = vec![0usize; cs.len()];
          for (k, &j) in cs.iter().enumerate() {
            let cost = (line(j) - row[i].0).abs();
            if i == 0 {
              b[k] = cost;
              continue;
            }
            for (kp, &jp) in cands[i - 1].iter().enumerate() {
              if jp < j && best[i - 1][kp] + cost < b[k] {
                b[k] = best[i - 1][kp] + cost;
                f[k] = kp;
              }
            }
          }
          best.push(b);
          from.push(f);
        }
        let Some(last) = best.last() else { continue };
        let Some((mut k, total)) = last
          .iter()
          .copied()
          .enumerate()
          .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
        else {
          continue;
        };
        if !total.is_finite() {
          continue;
        }
        for i in (0..row.len()).rev() {
          let Some(&j) = cands[i].get(k) else { break };
          self.lanes.insert(
            (fi, row[i].1),
            Lane {
              column: vertical,
              index: j as usize,
            },
          );
          k = from[i][k];
        }
      }
    }
  }

  fn find_preconnected(&mut self, board: &Board, root: &List) {
    struct Seg {
      net: u32,
      layer: usize,
      a: Point,
      b: Point,
    }
    let mut segs: Vec<Seg> = Vec::new();
    let mut vias: Vec<(u32, Point)> = Vec::new();
    for item in root.items.iter().filter_map(|n| n.as_list()) {
      let Some(net) = board.net_table.code_of(item).filter(|&n| n != 0) else {
        continue;
      };
      match item.head().as_deref() {
        Some("segment") => {
          let layer = item
            .child("layer")
            .and_then(|l| l.atom(1))
            .and_then(|l| self.layers.iter().position(|x| *x == *l));
          if let (Some(layer), Some(a), Some(b)) = (layer, xy(item.child("start")), xy(item.child("end"))) {
            segs.push(Seg { net, layer, a, b });
          }
        }
        Some("via") => {
          if let Some(at) = xy(item.child("at")) {
            vias.push((net, at));
          }
        }
        _ => {}
      }
    }
    if segs.is_empty() && vias.is_empty() {
      return;
    }
    let near = |p: Point, q: Point, tol: f64| (p - q).length() <= tol;
    // Connected components over tracks (0..segs.len()) and vias (after them).
    let n_s = segs.len();
    let mut parent: Vec<usize> = (0..n_s + vias.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
      loop {
        let p = parent.get(i).copied().unwrap_or(i);
        if p == i {
          return i;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
          *slot = gp;
        }
        i = gp;
      }
    }
    let union = |parent: &mut Vec<usize>, a: usize, b: usize| {
      let (ra, rb) = (find(parent, a), find(parent, b));
      if ra != rb
        && let Some(slot) = parent.get_mut(ra)
      {
        *slot = rb;
      }
    };
    for i in 0..n_s {
      for j in (i + 1)..n_s {
        let (Some(a), Some(b)) = (segs.get(i), segs.get(j)) else {
          continue;
        };
        if a.net == b.net
          && a.layer == b.layer
          && [a.a, a.b]
            .iter()
            .any(|&p| [b.a, b.b].iter().any(|&q| near(p, q, 0.001)))
        {
          union(&mut parent, i, j);
        }
      }
      for (k, &(vnet, at)) in vias.iter().enumerate() {
        let Some(a) = segs.get(i) else { continue };
        if a.net == vnet && (near(a.a, at, 0.01) || near(a.b, at, 0.01)) {
          union(&mut parent, i, n_s + k);
        }
      }
    }
    // Pads with attached copper: the components they touch.
    for (fi, f) in board.footprints.iter().enumerate() {
      for (pi, pad) in f.pads.iter().enumerate() {
        let Some(net) = pad.net.filter(|&n| n != 0) else {
          continue;
        };
        let shape = pad_copper(pad);
        let layers = self.pad_layer_indices(&pad.layers);
        let mut roots: Vec<usize> = Vec::new();
        for (i, s) in segs.iter().enumerate() {
          if s.net == net && layers.contains(&s.layer) && [s.a, s.b].iter().any(|&p| shape.sdf(p) <= 0.001) {
            roots.push(find(&mut parent, i));
          }
        }
        for (k, &(vnet, at)) in vias.iter().enumerate() {
          if vnet == net && shape.sdf(at) <= 0.001 {
            roots.push(find(&mut parent, n_s + k));
          }
        }
        if roots.is_empty() {
          continue;
        }
        roots.sort_unstable();
        roots.dedup();
        let mut seeds: Vec<(usize, usize, usize)> = Vec::new();
        for i in 0..n_s + vias.len() {
          if !roots.contains(&find(&mut parent, i)) {
            continue;
          }
          if let Some(s) = segs.get(i) {
            if let Some(bb) = BBox::from_points([s.a, s.b]) {
              let (x0, x1, y0, y1) = self.cell_range(bb, 0.02);
              for iy in y0..=y1 {
                for ix in x0..=x1 {
                  if crate::shape::seg_dist(self.centre(ix, iy), s.a, s.b) <= 0.02
                    && self.track_ok(s.layer, ix, iy, net)
                  {
                    seeds.push((s.layer, ix, iy));
                  }
                }
              }
            }
          } else if let Some(&(_, at)) = vias.get(i - n_s) {
            self.preconnected_via.insert((fi, pi));
            if let Some(bb) = BBox::from_points([at, at]) {
              let (x0, x1, y0, y1) = self.cell_range(bb, 0.02);
              for iy in y0..=y1 {
                for ix in x0..=x1 {
                  if (self.centre(ix, iy) - at).length() <= 0.02 {
                    for l in 0..self.layers.len() {
                      if self.track_ok(l, ix, iy, net) {
                        seeds.push((l, ix, iy));
                      }
                    }
                  }
                }
              }
            }
          }
        }
        seeds.sort_unstable();
        seeds.dedup();
        if !seeds.is_empty() {
          for &(_, ix, iy) in &seeds {
            let pt = (self.centre(ix, iy), net);
            if !self.seed_points.contains(&pt) {
              self.seed_points.push(pt);
            }
          }
          self.preconnected.insert((fi, pi), seeds);
        }
      }
    }
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
    // track 0.2 + clearance 0.2 + the project's max_error margin 0.005
    assert!((m.pitch - 0.405).abs() < 1e-12, "{}", m.pitch);
    assert_eq!(m.layers, vec!["F.Cu", "In1.Cu", "B.Cu"]);
  }

  #[test]
  fn blocking_a_via_site_forbids_it_for_every_net_once() {
    let (_, mut m) = build(&[]);
    let (ix, iy) = cell_at(&m, 12.0, 7.0);
    let at = m.centre(ix, iy);
    assert!(m.via_ok(ix, iy, 1));
    assert!(m.block_via_near(at));
    assert!(!m.via_ok(ix, iy, 1) && !m.via_ok(ix, iy, 2));
    assert!(!m.block_via_near(at), "already blocked");
    assert!(!m.block_via_near(Point::new(f64::NAN, 1.0)));
    assert!(!m.block_via_near(Point::new(-50.0, 1.0)));
    assert!(!m.block_via_near(Point::new(500.0, 1.0)));
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

  /// Regression (2026-10-13): C22.1 is a 3.5 x 1.6 mm roundrect (ratio 0.25).
  /// Access cells used to be chosen against its square outline, so a track
  /// could end in the rounded-off corner, outside the copper, and KiCad
  /// reported it dangling. Every access cell must lie on the true rounded
  /// copper, a half track-width in.
  #[test]
  fn access_cells_of_a_roundrect_pad_avoid_its_rounded_corners() {
    let src = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "C22")
    (pad "1" smd roundrect (at 10 5) (size 3.5 1.6) (layers "F.Cu") (roundrect_rratio 0.25) (net 1 "A")))
)"#;
    let doc = parse(src).unwrap();
    let board = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.0889, 0.0889, 0.25, 0.15).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &r, &p, &[]).unwrap();
    let a = m.pad_access(&board, 0, 0);
    assert!(a.cells.len() > 20);
    let pad = &board.footprints[0].pads[0];
    let truth = crate::shape::pad_copper(pad);
    for &(_, ix, iy) in &a.cells {
      let d = truth.sdf(m.centre(ix, iy));
      assert!(
        d <= -p.track_width / 2.0 + 1e-9,
        "cell ({ix},{iy}) is {d} from the copper edge"
      );
    }
    // The square corner (1.72, 0.78 from the centre) is a grid cell the old
    // code accepted; make sure the grid really has one there, and it is out.
    let corner = Point::new(10.0 + 1.72, 5.0 - 0.78);
    assert!(crate::shape::pad_obstacle(pad).sdf(corner) < 0.0);
    assert!(truth.sdf(corner) > 0.0);
  }

  /// Regression (round 7): a footprint-level `(clearance X)` raises the space
  /// kept around every pad of that footprint, in the model as in the verifier.
  #[test]
  fn a_footprint_clearance_override_widens_the_keep_out() {
    let src = |extra: &str| {
      format!(
        r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "J10") {extra}
    (pad "1" smd rect (at 10 5) (size 1 1) (layers "F.Cu") (net 1 "A"))))"#
      )
    };
    let build = |text: String| {
      let doc = parse(&text).unwrap();
      let board = Board::read(&doc).unwrap();
      let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
      r.min.copper_edge_clearance = 0.3;
      let p = r.classes[0].clone();
      (
        board.footprints[0].pads[0].clearance,
        RoutingModel::build(&doc, &board, &r, &p, &[]).unwrap(),
      )
    };
    let (none, plain) = build(src(""));
    let (some, wide) = build(src("(clearance 1.0)"));
    assert_eq!((none, some), (None, Some(1.0)));
    // A cell 0.8 mm beyond the pad edge (centre-line, other net): free under
    // the 0.2 rule (needs 0.3), blocked under the 1.0 override (needs 1.1).
    let at = |m: &RoutingModel| {
      let (ix, iy) = (
        ((10.0 + 0.5 + 0.8 - m.origin.x) / m.pitch).round() as usize,
        ((5.0 - m.origin.y) / m.pitch).round() as usize,
      );
      (m.centre(ix, iy).x - 10.5, m.track_ok(0, ix, iy, 2))
    };
    let (d1, free) = at(&plain);
    let (d2, blocked) = at(&wide);
    assert!(d1 > 0.5 && d2 > 0.5 && d1 < 1.1 && d2 < 1.1, "{d1} {d2}");
    assert!(free, "free under the plain rule");
    assert!(!blocked, "blocked under the override");
  }

  /// g2i: `+12V` is in a 0.25 mm "Power" class while the profile routed is
  /// Default (0.2 mm); spacing must satisfy the largest class so a Power net
  /// is never closer to its neighbours than its class allows.
  #[test]
  fn spacing_follows_the_largest_class_clearance() {
    let json = r#"{
      "board": {"design_settings": {"rules": {
        "min_clearance": 0.1, "min_track_width": 0.1, "min_via_diameter": 0.4,
        "min_through_hole_diameter": 0.2, "min_copper_edge_clearance": 0.3,
        "min_hole_clearance": 0.1, "min_hole_to_hole": 0.2}}},
      "net_settings": {"classes": [
        {"name": "Default", "clearance": 0.2, "track_width": 0.2, "via_diameter": 0.6, "via_drill": 0.3},
        {"name": "Power", "clearance": 0.5, "track_width": 0.5, "via_diameter": 0.8, "via_drill": 0.4}],
        "netclass_patterns": [{"netclass": "Power", "pattern": "B"}]}}"#;
    let r = DesignRules::from_json(json, "test").unwrap();
    let doc = parse(BOARD).unwrap();
    let board = Board::read(&doc).unwrap();
    let default = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &r, &default, &[]).unwrap();
    assert!((m.pitch - (0.2 + 0.5 + 0.005)).abs() < 1e-12, "pitch {}", m.pitch);
  }
}
