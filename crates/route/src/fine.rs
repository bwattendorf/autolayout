//! Fine-lattice negotiated router (M7.5, 2026-10-16).
//!
//! The grid router (`search`, `negotiate`) keeps tracks one pitch apart on a
//! grid of that pitch, so a pad that sits between grid lines may have no cell
//! to land on, and room that exists is lost to rounding. This router works on
//! a lattice `sub` times finer than the grid pitch (anchored at the grid's
//! origin, so every `sub`-th line is a grid line) and enforces the rules by
//! distance instead of by cell spacing:
//!
//! - a track centre-line keeps `track/2 + clearance` from foreign pad copper
//!   (distance maps, exact for the pad shapes);
//! - a track keeps `track + clearance` from another net's track, `via/2 +
//!   clearance + track/2` from another net's via, vias keep `via + clearance`
//!   from each other, and every via keeps hole-to-hole spacing from every other
//!   via (stamped as discs around the copper a route has laid);
//! - every distance is the rule plus `max_error` plus `slack`, rounded up to
//!   the lattice.
//!
//! Nets are routed as trees (a pad at a time, nearest first, from the tree
//! built so far), PathFinder style: every net is ripped up and re-routed in
//! turn, paying for cells that are inside another net's zone and for cells
//! that have been fought over, until no net is inside another's zone. A net
//! still overlapping at the end is dropped (reported failed), never written.
//! Plane nets need only a legal via. Same input, options and seed give the
//! same routes.
//!
//! Limits (first version): existing copper in the file and keepout zones are
//! not read (the caller must refuse a board that has them), the board outline
//! is its bounding box, only through vias.
//!
//! The lattice router is slower and hungrier than the grid router (about 100
//! bytes per lattice cell) and is meant for the nets the grid router cannot finish.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use autolayout_board::{Board, Pad, Point};

use crate::model::RoutingModel;
use crate::rules::DesignRules;
use crate::shape::{pad_copper, pad_obstacle};

/// Options of the fine router.
#[derive(Debug, Clone, Copy)]
pub struct FineParams {
  /// Lattice step is the grid pitch divided by this.
  pub sub: usize,
  pub iters: usize,
  /// Cost of a layer change, in lattice steps.
  pub via_cost: f32,
  /// Extra room kept on top of the rules and `max_error`, mm.
  pub slack: f64,
  /// History added to a fought-over cell per iteration.
  pub hist_inc: f32,
  pub seed: u64,
}

impl Default for FineParams {
  fn default() -> FineParams {
    FineParams {
      sub: 6,
      iters: 60,
      via_cost: 14.0,
      slack: 0.002,
      hist_inc: 1.0,
      seed: 1,
    }
  }
}

/// Copper of one net as the fine router laid it (millimetres).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FineRoute {
  pub net: u32,
  /// (routable layer index, from, to), collinear steps merged.
  pub segs: Vec<(usize, Point, Point)>,
  pub vias: Vec<Point>,
  /// Pads (footprint, pad) not connected.
  pub failed: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Default)]
pub struct FineResult {
  pub routes: Vec<FineRoute>,
  pub iterations: usize,
  /// Nets dropped at the end because they overlapped another.
  pub dropped: usize,
  pub nets_total: usize,
  pub nets_complete: usize,
  /// Lattice width and height in cells.
  pub lattice: (usize, usize),
  pub step: f64,
}

/// Net id of a pad that blocks every net (no net).
const NO_NET: u16 = u16::MAX;
const INF: f32 = 1.0e9;

/// Offsets (dx, dy) of lattice cells strictly within `radius` mm of a centre.
fn disc(radius: f64, step: f64) -> Vec<(i32, i32)> {
  let r = (radius / step).ceil() as i32;
  let mut out = Vec::new();
  for dy in -r..=r {
    for dx in -r..=r {
      if f64::from(dx).hypot(f64::from(dy)) * step < radius {
        out.push((dx, dy));
      }
    }
  }
  out
}

struct Lattice {
  w: usize,
  h: usize,
  nl: usize,
  step: f64,
  ox: f64,
  oy: f64,
  need_track: f64,
  need_via: f64,
  track: f64,
  via_drill: f64,
  h2h: f64,
  m: f64,
  // Nearest pad distance with its net, and nearest of another net, per layer.
  d1: Vec<Vec<f32>>,
  n1: Vec<Vec<u16>>,
  d2: Vec<Vec<f32>>,
  n2: Vec<Vec<u16>>,
  edge_ok: Vec<bool>,
  /// Cells where a via's hole would sit too near a pad's drill (any net, own included).
  via_block: Vec<bool>,
  // Zones stamped by routes: tracks keep tracks (tz) and vias (vz) away; hz is hole to hole.
  tz: Vec<Vec<i16>>,
  vz: Vec<Vec<i16>>,
  hz: Vec<i16>,
  hist: Vec<Vec<f32>>,
  k_tt: Vec<(i32, i32)>,
  k_vt: Vec<(i32, i32)>,
  k_vv: Vec<(i32, i32)>,
  k_vh: Vec<(i32, i32)>,
  /// Squared hole-to-hole distance in lattice cells (a net's own vias keep it from each other).
  vh_cells2: f64,
}

fn add_at(arr: &mut [i16], w: usize, h: usize, x: i32, y: i32, kernel: &[(i32, i32)], sign: i16) {
  for &(dx, dy) in kernel {
    let (xx, yy) = (x + dx, y + dy);
    if xx >= 0
      && yy >= 0
      && (xx as usize) < w
      && (yy as usize) < h
      && let Some(v) = arr.get_mut(yy as usize * w + xx as usize)
    {
      *v += sign;
    }
  }
}

impl Lattice {
  fn cells(&self) -> usize {
    self.w * self.h
  }

  fn point(&self, idx: usize) -> Point {
    Point::new(
      self.ox + (idx % self.w) as f64 * self.step,
      self.oy + (idx / self.w) as f64 * self.step,
    )
  }

  /// Distance from cell `idx` on layer `l` to the nearest pad of a net other than `q`.
  fn foreign(&self, l: usize, idx: usize, q: u16) -> f32 {
    let n1 = self.n1.get(l).and_then(|v| v.get(idx)).copied().unwrap_or(0);
    if n1 == q {
      self.d2.get(l).and_then(|v| v.get(idx)).copied().unwrap_or(INF)
    } else {
      self.d1.get(l).and_then(|v| v.get(idx)).copied().unwrap_or(INF)
    }
  }

  fn track_ok(&self, l: usize, idx: usize, q: u16) -> bool {
    self.edge_ok.get(idx).copied().unwrap_or(false) && f64::from(self.foreign(l, idx, q)) >= self.need_track
  }

  fn via_ok(&self, idx: usize, q: u16) -> bool {
    self.edge_ok.get(idx).copied().unwrap_or(false)
      && !self.via_block.get(idx).copied().unwrap_or(true)
      && self.hz.get(idx).copied().unwrap_or(1) == 0
      && (0..self.nl).all(|l| f64::from(self.foreign(l, idx, q)) >= self.need_via)
  }

  /// Stamp (`sign` = 1) or remove (`-1`) the zones of a net's copper.
  fn stamp_laid(&mut self, laid: &Laid, sign: i16) {
    let (w, h) = (self.w, self.h);
    for &(l, idx) in &laid.cells {
      let (x, y) = ((idx % w) as i32, (idx / w) as i32);
      if let Some(a) = self.tz.get_mut(l) {
        add_at(a, w, h, x, y, &self.k_tt, sign);
      }
      if let Some(a) = self.vz.get_mut(l) {
        add_at(a, w, h, x, y, &self.k_vt, sign);
      }
    }
    for &idx in &laid.vias {
      let (x, y) = ((idx % w) as i32, (idx / w) as i32);
      for l in 0..self.nl {
        if let Some(a) = self.tz.get_mut(l) {
          add_at(a, w, h, x, y, &self.k_vt, sign);
        }
        if let Some(a) = self.vz.get_mut(l) {
          add_at(a, w, h, x, y, &self.k_vv, sign);
        }
      }
      add_at(&mut self.hz, w, h, x, y, &self.k_vh, sign);
    }
  }

  /// How many cells of `laid` sit inside another net's zone (this net's own
  /// stamps must already be removed), and which.
  fn conflicts_of(&self, laid: &Laid) -> (usize, Vec<(usize, usize)>) {
    let mut n = 0usize;
    let mut at = Vec::new();
    for &(l, idx) in &laid.cells {
      if self.tz.get(l).and_then(|a| a.get(idx)).copied().unwrap_or(0) > 0 {
        n += 1;
        at.push((l, idx));
      }
    }
    for &idx in &laid.vias {
      let hit = (0..self.nl).any(|l| self.vz.get(l).and_then(|a| a.get(idx)).copied().unwrap_or(0) > 0)
        || self.hz.get(idx).copied().unwrap_or(0) > 0;
      if hit {
        n += 1;
        at.push((0, idx));
      }
    }
    (n, at)
  }
}

/// One connection of a net: the path to a pad, the vias on it, and (for a plane
/// tap that ended at a via an earlier tap of the net had placed) that via.
#[derive(Debug, Clone, Default)]
struct PathRec {
  pad: (usize, usize),
  cells: Vec<(usize, usize)>,
  vias: Vec<usize>,
  shares_via: Option<usize>,
}

/// What a net holds: its connections (for output and for dropping a tap on its
/// own), the union of their cells and vias (for the zones), the pads it missed.
#[derive(Debug, Clone, Default)]
struct Laid {
  cells: Vec<(usize, usize)>,
  paths: Vec<PathRec>,
  vias: Vec<usize>,
  failed: Vec<(usize, usize)>,
}

impl Laid {
  /// Rebuild `cells` and `vias` from the connections.
  fn refresh(&mut self) {
    self.cells = self.paths.iter().flat_map(|p| p.cells.iter().copied()).collect();
    self.vias = self.paths.iter().flat_map(|p| p.vias.iter().copied()).collect();
    self.cells.sort_unstable();
    self.cells.dedup();
    self.vias.sort_unstable();
    self.vias.dedup();
  }
}

struct Search {
  dist: Vec<f32>,
  prev: Vec<u32>,
  seen: Vec<u32>,
  epoch: u32,
}

/// Escape planning for one part: a window around its pads, each pad on a signal
/// net routed to the window's outer line (a plane net's pad to a legal via),
/// so the grid router can start from copper that is known to fit.
#[derive(Debug, Clone, Copy)]
pub struct EscapeParams {
  /// Footprint index of the part.
  pub part: usize,
  /// Window = the part's pad box grown by this, mm, snapped out to grid lines.
  pub margin: f64,
  /// Aim each escape at the side of the window facing the net's other pads:
  /// only exit nodes within this many mm of the nearest one to them.
  pub toward: Option<f64>,
}

/// Route every net of `board` with the fine lattice. `model` supplies the grid
/// (origin, pitch, extent), the signal layers, the plane layers and the net
/// class; `rules` the hole and edge rules.
pub fn route_fine(
  board: &Board,
  model: &RoutingModel,
  rules: &DesignRules,
  params: FineParams,
) -> FineResult {
  run(board, model, rules, params, None)
}

/// Plan the escapes of one part (see [`EscapeParams`]); the routes returned are
/// each pad's way out, in the same form as [`route_fine`]'s.
pub fn plan_escapes(
  board: &Board,
  model: &RoutingModel,
  rules: &DesignRules,
  params: FineParams,
  esc: EscapeParams,
) -> FineResult {
  run(board, model, rules, params, Some(esc))
}

fn run(
  board: &Board,
  model: &RoutingModel,
  rules: &DesignRules,
  params: FineParams,
  esc: Option<EscapeParams>,
) -> FineResult {
  let sub = params.sub.max(1);
  let step = model.pitch / sub as f64;
  // The lattice: the whole grid, or for an escape the window, whose corner is a grid node.
  let (ox, oy, w, h) = match esc.and_then(|e| escape_window(board, model, e)) {
    Some((gx0, gy0, gx1, gy1)) => (
      model.origin.x + gx0 as f64 * model.pitch,
      model.origin.y + gy0 as f64 * model.pitch,
      (gx1 - gx0) * sub + 1,
      (gy1 - gy0) * sub + 1,
    ),
    None => (
      model.origin.x,
      model.origin.y,
      model.nx.saturating_sub(1) * sub + 1,
      model.ny.saturating_sub(1) * sub + 1,
    ),
  };
  let nl = model.layers.len();
  let cells = w * h;
  let r = &model.profile;
  let m = model.margin + params.slack;
  let clr = rules
    .classes
    .iter()
    .map(|c| c.clearance)
    .fold(r.clearance.max(rules.min.clearance), f64::max);
  let tt = r.track_width + clr + m;
  let vt = r.via_diameter / 2.0 + clr + r.track_width / 2.0 + m;
  let vv = (r.via_diameter + clr).max(r.via_drill + rules.min.hole_to_hole) + m;
  let vh = r.via_drill + rules.min.hole_to_hole + m + 0.01;
  let mut lat = Lattice {
    w,
    h,
    nl,
    step,
    ox,
    oy,
    need_track: r.track_width / 2.0 + clr + m,
    need_via: r.via_diameter / 2.0 + clr + m,
    track: r.track_width,
    via_drill: r.via_drill,
    h2h: rules.min.hole_to_hole,
    m,
    d1: vec![vec![INF; cells]; nl],
    n1: vec![vec![0; cells]; nl],
    d2: vec![vec![INF; cells]; nl],
    n2: vec![vec![0; cells]; nl],
    edge_ok: vec![true; cells],
    via_block: vec![false; cells],
    tz: vec![vec![0; cells]; nl],
    vz: vec![vec![0; cells]; nl],
    hz: vec![0; cells],
    hist: vec![vec![0.0; cells]; nl],
    k_tt: disc(tt, step),
    k_vt: disc(vt, step),
    k_vv: disc(vv, step),
    k_vh: disc(vh, step),
    vh_cells2: (vh / step) * (vh / step),
  };
  // Net ids: net code -> 1.. (0 = none).
  let mut id_of: HashMap<u32, u16> = HashMap::new();
  for p in board.footprints.iter().flat_map(|f| f.pads.iter()) {
    if let Some(n) = p.net.filter(|&n| n != 0) {
      let next = (id_of.len() + 1).min(usize::from(NO_NET) - 1) as u16;
      id_of.entry(n).or_insert(next);
    }
  }
  let reach = lat.need_via.max(lat.need_track) + 0.4;
  build_obstacles(&mut lat, board, model, &id_of, reach);
  if let Some(o) = board.outline {
    let edge = rules.min.copper_edge_clearance + r.track_width / 2.0;
    for iy in 0..h {
      for ix in 0..w {
        let p = lat.point(iy * w + ix);
        if (p.x < o.min.x + edge || p.x > o.max.x - edge || p.y < o.min.y + edge || p.y > o.max.y - edge)
          && let Some(e) = lat.edge_ok.get_mut(iy * w + ix)
        {
          *e = false;
        }
      }
    }
  }
  let plane_nets: Vec<u32> = model.planes.iter().map(|p| p.net).collect();
  let mut by_net = board.pads_by_net();
  let min_pads = if esc.is_some() { 1 } else { 2 };
  if let Some(e) = esc {
    // Only this part's pads are routed, each to the edge of the window.
    by_net = by_net
      .into_iter()
      .filter_map(|(n, pads)| {
        let mine: Vec<(usize, usize)> = pads.into_iter().filter(|&(f, _)| f == e.part).collect();
        (!mine.is_empty()).then_some((n, mine))
      })
      .collect();
  }
  let mut nets: Vec<u32> = by_net
    .iter()
    .filter(|&(&n, pads)| n != 0 && pads.len() >= min_pads && id_of.contains_key(&n))
    .map(|(&n, _)| n)
    .collect();
  nets.sort_unstable();
  let exits: HashMap<u32, HashSet<usize>> = match esc {
    Some(e) => nets
      .iter()
      .map(|&n| (n, exit_states(&lat, board, model, &id_of, n, e)))
      .collect(),
    None => HashMap::new(),
  };
  let mut search = Search {
    dist: vec![INF; nl * cells],
    prev: vec![u32::MAX; nl * cells],
    seen: vec![0; nl * cells],
    epoch: 0,
  };
  let mut laid: Vec<Laid> = vec![Laid::default(); nets.len()];
  let mut order: Vec<usize> = (0..nets.len()).collect();
  let mut rng = params.seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
  let mut pres = 0.4f32;
  let mut iterations = 0usize;
  let mut best_score = usize::MAX;
  let mut best: Vec<Laid> = laid.clone();
  for it in 0..params.iters.max(1) {
    iterations = it + 1;
    if it > 0 {
      // Deterministic shuffle (xorshift).
      for i in (1..order.len()).rev() {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let j = (rng % (i as u64 + 1)) as usize;
        order.swap(i, j);
      }
    }
    for &k in &order {
      let (Some(&net), Some(old)) = (nets.get(k), laid.get(k).cloned()) else {
        continue;
      };
      let Some(&q) = id_of.get(&net) else { continue };
      lat.stamp_laid(&old, -1);
      let pads = by_net.get(&net).cloned().unwrap_or_default();
      let new = route_one(
        &lat,
        &mut search,
        board,
        model,
        q,
        &pads,
        plane_nets.contains(&net),
        pres,
        params.via_cost,
        exits.get(&net),
      );
      lat.stamp_laid(&new, 1);
      if let Some(slot) = laid.get_mut(k) {
        *slot = new;
      }
    }
    // Conflicts: a net's copper inside another net's zone.
    let mut conflicts = 0usize;
    let mut fought: Vec<(usize, usize)> = Vec::new();
    for l in &laid {
      lat.stamp_laid(l, -1);
      let (c, at) = lat.conflicts_of(l);
      conflicts += c;
      fought.extend(at);
      lat.stamp_laid(l, 1);
    }
    for (layer, idx) in fought {
      let (x, y) = ((idx % w) as i64, (idx / w) as i64);
      for dy in -2i64..=2 {
        for dx in -2i64..=2 {
          let (xx, yy) = (x + dx, y + dy);
          if xx >= 0
            && yy >= 0
            && (xx as usize) < w
            && (yy as usize) < h
            && let Some(v) = lat
              .hist
              .get_mut(layer)
              .and_then(|a| a.get_mut(yy as usize * w + xx as usize))
          {
            *v += params.hist_inc;
          }
        }
      }
    }
    let failed_pads: usize = laid.iter().map(|l| l.failed.len()).sum();
    let score = conflicts * 1000 + failed_pads;
    if score < best_score {
      best_score = score;
      best = laid.clone();
    }
    if conflicts == 0 && failed_pads == 0 {
      break;
    }
    pres = (pres * 1.35).min(50.0);
  }
  // Keep what overlaps nothing already kept (in net order). A signal net is kept
  // whole or dropped; a plane net's taps are independent, so only the conflicting
  // taps (and the taps that shared a dropped tap's via) are dropped.
  for l in &laid {
    lat.stamp_laid(l, -1);
  }
  let mut kept: Vec<Laid> = Vec::with_capacity(best.len());
  let mut dropped = 0usize;
  for (k, l) in best.iter().enumerate() {
    let net = nets.get(k).copied().unwrap_or(0);
    let pads = by_net.get(&net).cloned().unwrap_or_default();
    if plane_nets.contains(&net) {
      let mut out = Laid {
        failed: l.failed.clone(),
        ..Laid::default()
      };
      let mut live_vias: HashSet<usize> = HashSet::new();
      for rec in &l.paths {
        let one = Laid {
          cells: rec.cells.clone(),
          vias: rec.vias.clone(),
          ..Laid::default()
        };
        let needs_ok = rec.shares_via.is_none_or(|v| live_vias.contains(&v));
        let (c, _) = lat.conflicts_of(&one);
        if c == 0 && needs_ok {
          lat.stamp_laid(&one, 1);
          live_vias.extend(rec.vias.iter().copied());
          out.paths.push(rec.clone());
        } else {
          out.failed.push(rec.pad);
        }
      }
      out.refresh();
      if out.failed.len() > l.failed.len() {
        dropped += 1;
      }
      kept.push(out);
      continue;
    }
    let (c, _) = lat.conflicts_of(l);
    if c == 0 {
      lat.stamp_laid(l, 1);
      kept.push(l.clone());
    } else {
      dropped += 1;
      kept.push(Laid {
        failed: pads,
        ..Laid::default()
      });
    }
  }
  let mut routes = Vec::new();
  let mut complete = 0usize;
  for (k, l) in kept.iter().enumerate() {
    let Some(&net) = nets.get(k) else { continue };
    let fr = to_route(&lat, net, l);
    if fr.failed.is_empty() {
      complete += 1;
    }
    routes.push(fr);
  }
  FineResult {
    routes,
    iterations,
    dropped,
    nets_total: nets.len(),
    nets_complete: complete,
    lattice: (w, h),
    step,
  }
}

/// Grid-line window (x0, y0, x1, y1 as grid indices) around the part's pads grown by the margin.
fn escape_window(
  board: &Board,
  model: &RoutingModel,
  e: EscapeParams,
) -> Option<(usize, usize, usize, usize)> {
  let fp = board.footprints.get(e.part)?;
  let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
  for p in &fp.pads {
    let bb = pad_copper(p).bbox()?;
    x0 = x0.min(bb.min.x);
    y0 = y0.min(bb.min.y);
    x1 = x1.max(bb.max.x);
    y1 = y1.max(bb.max.y);
  }
  if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
    return None;
  }
  let lo = |v: f64, o: f64| ((v - e.margin - o) / model.pitch).floor().max(0.0) as usize;
  let hi = |v: f64, o: f64, n: usize| {
    ((((v + e.margin - o) / model.pitch).ceil().max(0.0)) as usize).min(n.saturating_sub(1))
  };
  let (gx0, gy0) = (lo(x0, model.origin.x), lo(y0, model.origin.y));
  let (gx1, gy1) = (hi(x1, model.origin.x, model.nx), hi(y1, model.origin.y, model.ny));
  (gx1 > gx0 && gy1 > gy0).then_some((gx0, gy0, gx1, gy1))
}

/// The states (layer * cells + cell) an escape of net `n` may end on: grid nodes on the window's
/// outer line where a track may stand, narrowed with `toward` to those nearest the net's other pads.
fn exit_states(
  lat: &Lattice,
  board: &Board,
  model: &RoutingModel,
  id_of: &HashMap<u32, u16>,
  n: u32,
  e: EscapeParams,
) -> HashSet<usize> {
  let q = id_of.get(&n).copied().unwrap_or(0);
  let sub = (model.pitch / lat.step).round().max(1.0) as usize;
  let cells = lat.cells();
  let mut nodes: Vec<(usize, usize)> = Vec::new();
  for iy in 0..lat.h {
    for ix in 0..lat.w {
      let ring = ix == 0 || iy == 0 || ix + 1 == lat.w || iy + 1 == lat.h;
      if ring && ix % sub == 0 && iy % sub == 0 {
        for l in 0..lat.nl {
          if lat.track_ok(l, iy * lat.w + ix, q) {
            nodes.push((l, iy * lat.w + ix));
          }
        }
      }
    }
  }
  if let Some(slack) = e.toward {
    let others: Vec<Point> = board
      .footprints
      .iter()
      .enumerate()
      .filter(|&(f, _)| f != e.part)
      .flat_map(|(_, fp)| fp.pads.iter())
      .filter(|p| p.net == Some(n))
      .map(|p| p.at)
      .collect();
    if !others.is_empty() {
      let k = others.len() as f64;
      let dest = Point::new(
        others.iter().map(|p| p.x).sum::<f64>() / k,
        others.iter().map(|p| p.y).sum::<f64>() / k,
      );
      let near = |&(_, idx): &(usize, usize)| (lat.point(idx) - dest).length();
      let best = nodes.iter().map(near).fold(f64::INFINITY, f64::min);
      nodes.retain(|c| near(c) <= best + slack);
    }
  }
  nodes.into_iter().map(|(l, idx)| l * cells + idx).collect()
}

/// Nearest-pad distance maps (nearest net, and nearest of any other net) from
/// every pad's obstacle shape.
fn build_obstacles(
  lat: &mut Lattice,
  board: &Board,
  model: &RoutingModel,
  id_of: &HashMap<u32, u16>,
  reach: f64,
) {
  let (w, h, step) = (lat.w, lat.h, lat.step);
  for fp in &board.footprints {
    for pad in &fp.pads {
      if pad.kind == "np_thru_hole" && pad.drill.is_none() {
        continue;
      }
      let q = pad.net.and_then(|n| id_of.get(&n).copied()).unwrap_or(NO_NET);
      if let Some(d) = pad.drill {
        // A via hole keeps hole-to-hole spacing from this pad's drill, whatever the net.
        let r = d.x.max(d.y) / 2.0 + lat.via_drill / 2.0 + lat.h2h + lat.m + 0.01;
        let ix0 = ((pad.at.x - r - lat.ox) / step).floor().max(0.0) as usize;
        let ix1 = (((pad.at.x + r - lat.ox) / step).ceil().max(0.0) as usize).min(w.saturating_sub(1));
        let iy0 = ((pad.at.y - r - lat.oy) / step).floor().max(0.0) as usize;
        let iy1 = (((pad.at.y + r - lat.oy) / step).ceil().max(0.0) as usize).min(h.saturating_sub(1));
        for iy in iy0..=iy1 {
          for ix in ix0..=ix1 {
            if (lat.point(iy * w + ix) - pad.at).length() < r
              && let Some(b) = lat.via_block.get_mut(iy * w + ix)
            {
              *b = true;
            }
          }
        }
      }
      let shape = pad_obstacle(pad);
      let Some(bb) = shape.bbox() else { continue };
      let layers = model.pad_layer_indices(&pad.layers);
      let ix0 = ((bb.min.x - reach - lat.ox) / step).floor().max(0.0) as usize;
      let ix1 = (((bb.max.x + reach - lat.ox) / step).ceil().max(0.0) as usize).min(w.saturating_sub(1));
      let iy0 = ((bb.min.y - reach - lat.oy) / step).floor().max(0.0) as usize;
      let iy1 = (((bb.max.y + reach - lat.oy) / step).ceil().max(0.0) as usize).min(h.saturating_sub(1));
      for iy in iy0..=iy1 {
        for ix in ix0..=ix1 {
          let idx = iy * w + ix;
          let d = shape.distance(lat.point(idx)) as f32;
          if f64::from(d) > reach {
            continue;
          }
          for &l in &layers {
            let Some(cur) = read4(lat, l, idx) else { continue };
            let (mut d1, mut n1, mut d2, mut n2) = cur;
            if n1 == q {
              d1 = d1.min(d);
            } else if n2 == q {
              if d < d2 {
                d2 = d;
                if d2 < d1 {
                  std::mem::swap(&mut d1, &mut d2);
                  std::mem::swap(&mut n1, &mut n2);
                }
              }
            } else if d < d1 {
              (d2, n2) = (d1, n1);
              (d1, n1) = (d, q);
            } else if d < d2 {
              (d2, n2) = (d, q);
            }
            write4(lat, l, idx, (d1, n1, d2, n2));
          }
        }
      }
    }
  }
}

fn read4(lat: &Lattice, l: usize, idx: usize) -> Option<(f32, u16, f32, u16)> {
  Some((
    *lat.d1.get(l)?.get(idx)?,
    *lat.n1.get(l)?.get(idx)?,
    *lat.d2.get(l)?.get(idx)?,
    *lat.n2.get(l)?.get(idx)?,
  ))
}

fn write4(lat: &mut Lattice, l: usize, idx: usize, v: (f32, u16, f32, u16)) {
  if let Some(x) = lat.d1.get_mut(l).and_then(|a| a.get_mut(idx)) {
    *x = v.0;
  }
  if let Some(x) = lat.n1.get_mut(l).and_then(|a| a.get_mut(idx)) {
    *x = v.1;
  }
  if let Some(x) = lat.d2.get_mut(l).and_then(|a| a.get_mut(idx)) {
    *x = v.2;
  }
  if let Some(x) = lat.n2.get_mut(l).and_then(|a| a.get_mut(idx)) {
    *x = v.3;
  }
}

/// Lattice cells a track may start on or end at inside `pad` on its layers:
/// the track end fully on copper, else any cell inside the pad.
fn pad_cells(lat: &Lattice, model: &RoutingModel, pad: &Pad, q: u16) -> Vec<(usize, usize)> {
  let shape = pad_copper(pad);
  let Some(bb) = shape.bbox() else { return Vec::new() };
  let step = lat.step;
  let ix0 = ((bb.min.x - lat.ox) / step).floor().max(0.0) as usize;
  let ix1 = (((bb.max.x - lat.ox) / step).ceil().max(0.0) as usize).min(lat.w.saturating_sub(1));
  let iy0 = ((bb.min.y - lat.oy) / step).floor().max(0.0) as usize;
  let iy1 = (((bb.max.y - lat.oy) / step).ceil().max(0.0) as usize).min(lat.h.saturating_sub(1));
  let layers = model.pad_layer_indices(&pad.layers);
  for inset in [lat.track / 2.0, 0.0] {
    let mut out = Vec::new();
    for iy in iy0..=iy1 {
      for ix in ix0..=ix1 {
        let idx = iy * lat.w + ix;
        if shape.sdf(lat.point(idx)) <= -inset + 1e-9 {
          for &l in &layers {
            if lat.track_ok(l, idx, q) {
              out.push((l, idx));
            }
          }
        }
      }
    }
    if !out.is_empty() {
      return out;
    }
  }
  Vec::new()
}

enum Goal {
  /// Reach any of these cells (a pad); the point is the pad centre, for the heuristic.
  Cells(Vec<(usize, usize)>, Point),
  /// Reach any cell where a via may stand (plane taps); the via is placed there.
  ViaSite,
  /// Reach any of these states (layer * cells + cell): an escape's exit nodes.
  Exit(HashSet<usize>),
}

#[allow(clippy::too_many_arguments)]
fn route_one(
  lat: &Lattice,
  search: &mut Search,
  board: &Board,
  model: &RoutingModel,
  q: u16,
  pads: &[(usize, usize)],
  plane: bool,
  pres: f32,
  via_cost: f32,
  exit: Option<&HashSet<usize>>,
) -> Laid {
  let mut laid = Laid::default();
  let mut starts: Vec<PadStart> = Vec::new();
  for &(fi, pi) in pads {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      continue;
    };
    let c = pad_cells(lat, model, pad, q);
    if c.is_empty() {
      laid.failed.push((fi, pi));
    } else {
      starts.push((c, (fi, pi), pad.at));
    }
  }
  if starts.is_empty() {
    return laid;
  }
  if plane {
    // Each pad needs only a legal via reachable from it (or one of this net's vias already placed).
    for (cells, pad_id, _) in &starts {
      let own: Vec<usize> = laid.paths.iter().flat_map(|p| p.vias.iter().copied()).collect();
      match find_path(lat, search, q, cells, &Goal::ViaSite, &own, pres, via_cost) {
        Some((path, via_at)) => add_path(&mut laid, *pad_id, path, via_at, &own),
        None => laid.failed.push(*pad_id),
      }
    }
  } else if let Some(exit) = exit {
    // Escape: each pad on its own to the window's edge.
    for (cells, pad_id, _) in &starts {
      let own: Vec<usize> = laid.paths.iter().flat_map(|p| p.vias.iter().copied()).collect();
      match find_path(
        lat,
        search,
        q,
        cells,
        &Goal::Exit(exit.clone()),
        &own,
        pres,
        via_cost,
      ) {
        Some((path, via_at)) => add_path(&mut laid, *pad_id, path, via_at, &own),
        None => laid.failed.push(*pad_id),
      }
    }
  } else {
    // Signal net: grow a tree from the first pad, nearest pad next.
    let mut tree: Vec<(usize, usize)> = starts.first().map(|s| s.0.clone()).unwrap_or_default();
    let mut remaining: Vec<usize> = (1..starts.len()).collect();
    while !remaining.is_empty() {
      let tree_pts: Vec<Point> = tree.iter().map(|&(_, idx)| lat.point(idx)).collect();
      let near = |k: usize| {
        starts.get(k).map_or(f64::INFINITY, |s| {
          tree_pts
            .iter()
            .map(|p| (*p - s.2).length())
            .fold(f64::INFINITY, f64::min)
        })
      };
      let pick = remaining
        .iter()
        .copied()
        .min_by(|&a, &b| near(a).total_cmp(&near(b)).then(a.cmp(&b)))
        .unwrap_or(0);
      remaining.retain(|&k| k != pick);
      let Some((goal_cells, pad_id, pad_at)) = starts.get(pick) else {
        continue;
      };
      let own: Vec<usize> = laid.paths.iter().flat_map(|p| p.vias.iter().copied()).collect();
      match find_path(
        lat,
        search,
        q,
        &tree,
        &Goal::Cells(goal_cells.clone(), *pad_at),
        &own,
        pres,
        via_cost,
      ) {
        Some((path, via_at)) => {
          tree.extend(path.iter().copied());
          tree.extend(goal_cells.iter().copied());
          add_path(&mut laid, *pad_id, path, via_at, &own);
        }
        None => laid.failed.push(*pad_id),
      }
    }
  }
  laid.refresh();
  laid
}

fn add_path(
  laid: &mut Laid,
  pad: (usize, usize),
  path: Vec<(usize, usize)>,
  via_at: Option<usize>,
  own_vias: &[usize],
) {
  let mut vias = Vec::new();
  for (k, &(_, idx)) in path.iter().enumerate() {
    if k > 0
      && let (Some(&(pl, pidx)), Some(&(l, _))) = (path.get(k - 1), path.get(k))
      && pidx == idx
      && pl != l
    {
      vias.push(idx);
    }
  }
  if let Some(v) = via_at {
    vias.push(v);
  }
  // A plane tap that ended at a via of this net already placed depends on that via.
  let shares_via = path
    .last()
    .map(|&(_, i)| i)
    .filter(|i| via_at.is_none() && own_vias.contains(i));
  laid.paths.push(PathRec {
    pad,
    cells: path,
    vias,
    shares_via,
  });
}

/// A pad's start cells, its (footprint, pad) and its centre.
type PadStart = (Vec<(usize, usize)>, (usize, usize), Point);
/// A found path and, for a via-site goal, the cell where the via goes.
type Found = (Vec<(usize, usize)>, Option<usize>);

/// A* from `sources` to the goal; returns the path cells (layer, cell) and,
/// for `ViaSite`, the cell where the via goes.
#[allow(clippy::too_many_arguments)]
fn find_path(
  lat: &Lattice,
  s: &mut Search,
  q: u16,
  sources: &[(usize, usize)],
  goal: &Goal,
  own_vias: &[usize],
  pres: f32,
  via_cost: f32,
) -> Option<Found> {
  let (w, h) = (lat.w, lat.h);
  let cells = lat.cells();
  s.epoch = s.epoch.wrapping_add(1);
  if s.epoch == 0 {
    s.seen.iter_mut().for_each(|v| *v = 0);
    s.epoch = 1;
  }
  let goal_set: HashSet<usize> = match goal {
    Goal::Cells(c, _) => c.iter().map(|&(l, i)| l * cells + i).collect(),
    Goal::ViaSite => HashSet::new(),
    Goal::Exit(e) => e.clone(),
  };
  // A source that is already a goal: the pad is touched by the tree.
  if let Goal::Cells(c, _) = goal
    && let Some(&hit) = c.iter().find(|x| sources.contains(x))
  {
    return Some((vec![hit], None));
  }
  let (gx, gy) = match goal {
    Goal::Cells(_, p) => ((p.x - lat.ox) / lat.step, (p.y - lat.oy) / lat.step),
    Goal::ViaSite | Goal::Exit(_) => (0.0, 0.0),
  };
  let exit_goal = matches!(goal, Goal::Exit(_));
  let via_goal = matches!(goal, Goal::ViaSite);
  // A via may stand at `idx` if the rules allow it and it keeps hole spacing from this net's own vias.
  let via_here = |idx: usize| -> bool {
    lat.via_ok(idx, q)
      && own_vias.iter().all(|&v| {
        let (dx, dy) = (
          (idx % w) as f64 - (v % w) as f64,
          (idx / w) as f64 - (v / w) as f64,
        );
        dx * dx + dy * dy >= lat.vh_cells2
      })
  };
  let hfun = |idx: usize| -> f32 {
    if via_goal {
      return 0.0;
    }
    let (x, y) = ((idx % w) as f64, (idx / w) as f64);
    if exit_goal {
      // Distance in steps to the window's edge (admissible for any subset of it).
      return x.min(y).min((w - 1) as f64 - x).min((h - 1) as f64 - y).max(0.0) as f32;
    }
    let (dx, dy) = ((x - gx).abs(), (y - gy).abs());
    let oct = dx.max(dy) + (std::f64::consts::SQRT_2 - 1.0) * dx.min(dy);
    (oct - 4.0).max(0.0) as f32
  };
  let key = |f: f32| -> u32 { (f.max(0.0) * 16.0).min(4.0e9) as u32 };
  let mut heap: BinaryHeap<(Reverse<u32>, Reverse<u32>)> = BinaryHeap::new();
  for &(l, idx) in sources {
    let st = l * cells + idx;
    if let (Some(d), Some(sn), Some(p)) = (s.dist.get_mut(st), s.seen.get_mut(st), s.prev.get_mut(st)) {
      *d = 0.0;
      *sn = s.epoch;
      *p = u32::MAX;
      heap.push((Reverse(key(hfun(idx))), Reverse(st as u32)));
    }
  }
  let nb: [(i32, i32, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, std::f32::consts::SQRT_2),
    (1, -1, std::f32::consts::SQRT_2),
    (-1, 1, std::f32::consts::SQRT_2),
    (-1, -1, std::f32::consts::SQRT_2),
  ];
  let mut found: Option<usize> = None;
  while let Some((_, Reverse(st32))) = heap.pop() {
    let st = st32 as usize;
    let (l, idx) = (st / cells, st % cells);
    let g = s.dist.get(st).copied().unwrap_or(INF);
    if via_goal {
      if own_vias.contains(&idx) || via_here(idx) {
        found = Some(st);
        break;
      }
    } else if goal_set.contains(&st) {
      found = Some(st);
      break;
    }
    let (x, y) = ((idx % w) as i32, (idx / w) as i32);
    for &(dx, dy, c) in &nb {
      let (xx, yy) = (x + dx, y + dy);
      if xx < 0 || yy < 0 || xx >= w as i32 || yy >= h as i32 {
        continue;
      }
      let nidx = yy as usize * w + xx as usize;
      if !lat.track_ok(l, nidx, q) {
        continue;
      }
      let mult = 1.0
        + pres
          * f32::from(
            lat
              .tz
              .get(l)
              .and_then(|a| a.get(nidx))
              .copied()
              .unwrap_or(0)
              .max(0),
          )
        + lat.hist.get(l).and_then(|a| a.get(nidx)).copied().unwrap_or(0.0);
      relax(s, l * cells + nidx, g + c * mult, st, &mut heap, hfun(nidx), &key);
    }
    if lat.nl > 1 && via_here(idx) {
      let vmult = pres
        * (0..lat.nl)
          .map(|ll| {
            f32::from(
              lat
                .vz
                .get(ll)
                .and_then(|a| a.get(idx))
                .copied()
                .unwrap_or(0)
                .max(0),
            )
          })
          .sum::<f32>();
      for l2 in 0..lat.nl {
        if l2 != l && lat.track_ok(l2, idx, q) {
          let mult = 1.0
            + pres
              * f32::from(
                lat
                  .tz
                  .get(l2)
                  .and_then(|a| a.get(idx))
                  .copied()
                  .unwrap_or(0)
                  .max(0),
              )
            + lat.hist.get(l2).and_then(|a| a.get(idx)).copied().unwrap_or(0.0);
          relax(
            s,
            l2 * cells + idx,
            g + via_cost + vmult + mult,
            st,
            &mut heap,
            hfun(idx),
            &key,
          );
        }
      }
    }
  }
  let end = found?;
  let mut path = vec![(end / cells, end % cells)];
  let mut cur = end;
  while let Some(&p) = s.prev.get(cur) {
    if p == u32::MAX {
      break;
    }
    cur = p as usize;
    path.push((cur / cells, cur % cells));
  }
  path.reverse();
  let via_at = (via_goal && !own_vias.contains(&(end % cells))).then_some(end % cells);
  Some((path, via_at))
}

fn relax(
  s: &mut Search,
  nst: usize,
  ng: f32,
  from: usize,
  heap: &mut BinaryHeap<(Reverse<u32>, Reverse<u32>)>,
  hv: f32,
  key: &dyn Fn(f32) -> u32,
) {
  let epoch = s.epoch;
  let better = s.seen.get(nst).is_some_and(|&e| e != epoch) || s.dist.get(nst).is_some_and(|&d| ng < d);
  if better {
    if let (Some(d), Some(sn), Some(p)) = (s.dist.get_mut(nst), s.seen.get_mut(nst), s.prev.get_mut(nst)) {
      *d = ng;
      *sn = epoch;
      *p = from as u32;
    }
    heap.push((Reverse(key(ng + hv)), Reverse(nst as u32)));
  }
}

/// A net's paths as millimetre segments (steps of one direction merged) and vias.
fn to_route(lat: &Lattice, net: u32, laid: &Laid) -> FineRoute {
  let mut segs: Vec<(usize, Point, Point)> = Vec::new();
  for rec in &laid.paths {
    let path = &rec.cells;
    let mut k = 0usize;
    while k + 1 < path.len() {
      let (Some(&(l, a)), Some(&(l2, b))) = (path.get(k), path.get(k + 1)) else {
        break;
      };
      if l != l2 || a == b {
        k += 1;
        continue;
      }
      let dir = (
        (b % lat.w) as i64 - (a % lat.w) as i64,
        (b / lat.w) as i64 - (a / lat.w) as i64,
      );
      let mut end = b;
      let mut e = k + 1;
      while let (Some(&(l3, c)), Some(&(_, d))) = (path.get(e), path.get(e + 1)) {
        let step = (
          (d % lat.w) as i64 - (c % lat.w) as i64,
          (d / lat.w) as i64 - (c / lat.w) as i64,
        );
        let same_layer = path.get(e + 1).is_some_and(|&(l4, _)| l4 == l3);
        if l3 == l && same_layer && step == dir {
          end = d;
          e += 1;
        } else {
          break;
        }
      }
      segs.push((l, lat.point(a), lat.point(end)));
      k = e;
    }
  }
  FineRoute {
    net,
    segs,
    vias: laid.vias.iter().map(|&i| lat.point(i)).collect(),
    failed: laid.failed.clone(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::RoutingModel;
  use crate::rules::DesignRules;
  use autolayout_sexpr::parse;

  /// 30 x 12 mm. Net A has pads at (5,6), (25,6) and (15,10); net B a
  /// through-hole pad in the way at (15,6) and a partner at (15,2); net C two
  /// pads on opposite sides of a B wall that only a via can pass.
  const BOARD: &str = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C")
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts"))
  (gr_line (start 30 0) (end 30 12) (layer "Edge.Cuts"))
  (gr_line (start 30 12) (end 0 12) (layer "Edge.Cuts"))
  (gr_line (start 0 12) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 5 6) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 25 6) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "3" smd rect (at 15 10) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "4" thru_hole circle (at 15 6) (size 1.6 1.6) (drill 0.8) (layers "*.Cu") (net 2 "B"))
    (pad "5" smd rect (at 15 2) (size 0.8 0.8) (layers "F.Cu") (net 2 "B"))
    (pad "6" smd rect (at 20 3) (size 0.8 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "7" smd rect (at 20 9) (size 0.8 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "8" smd rect (at 20 4.5) (size 6 0.5) (layers "F.Cu") (net 2 "B"))
    (pad "9" smd rect (at 20 7.5) (size 6 0.5) (layers "F.Cu") (net 2 "B")))
  (embedded_fonts no)
)"#;

  fn setup() -> (autolayout_sexpr::Document, Board, RoutingModel, DesignRules) {
    let doc = parse(BOARD).unwrap();
    let board = Board::read(&doc).unwrap();
    let mut rules = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    rules.min.copper_edge_clearance = 0.5;
    let p = rules.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &rules, &p, &[]).unwrap();
    (doc, board, m, rules)
  }

  #[test]
  fn routes_a_multi_pin_net_around_and_over_and_the_file_verifier_agrees() {
    let (mut doc, board, m, rules) = setup();
    let res = route_fine(&board, &m, &rules, FineParams::default());
    assert_eq!(res.nets_total, 3);
    assert_eq!(
      res.nets_complete,
      3,
      "{:?}",
      res.routes.iter().map(|r| &r.failed).collect::<Vec<_>>()
    );
    assert_eq!(res.dropped, 0);
    // Net C crosses the B wall: it needs a via.
    let c = res.routes.iter().find(|r| r.net == 3).unwrap();
    assert!(!c.vias.is_empty(), "C must use a via to cross the B wall");
    crate::write::add_routes_fine(&mut doc, &m, &board, &res.routes).unwrap();
    let text = doc.to_string();
    let back = parse(&text).unwrap();
    assert_eq!(back.to_string(), text);
    let b2 = Board::read(&back).unwrap();
    let report = crate::verify::verify(&back, &b2, &rules, &[]);
    assert!(report.clean(), "{report:?}");
    assert_eq!(report.unconnected, 0, "{report:?}");
  }

  #[test]
  fn same_input_and_seed_give_the_same_copper() {
    let (_, board, m, rules) = setup();
    let a = route_fine(&board, &m, &rules, FineParams::default());
    let b = route_fine(&board, &m, &rules, FineParams::default());
    assert_eq!(a.routes, b.routes);
  }
}
