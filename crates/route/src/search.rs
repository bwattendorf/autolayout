//! One net at a time: deterministic A* on the 3-D grid (layer, x, y) (M7.2).
//!
//! A multi-pin net grows a tree. The tree starts as the cells of one pad; each
//! search starts from every cell already in the tree (cost 0) and ends at the
//! first cell of any pad still unconnected, so the next pad joined is the
//! cheapest one to reach. The path is added to the tree and the search
//! repeats. Moves are the four grid neighbours on a layer (stepping against a
//! layer's preferred direction costs more) and a through-via to another
//! layer. Costs are integers and ties break on state index, so the same board
//! gives the same routes (CLAUDE.md rule 4).
//!
//! Publication: maze search is Lee, "An Algorithm for Path Connections and
//! Its Applications", IRE Trans. Electronic Computers, 1961; A* is Hart,
//! Nilsson & Raphael, 1968. The heuristic here is Manhattan distance to the
//! nearest unconnected pad's cell box, never above the true cost because every
//! step costs at least `STEP`.
//!
//! Pads whose copper contains no grid cell (common when the pitch is coarse,
//! see M7.1) get a *stub*: the nearest usable cells within `1.5 × pitch` are
//! the terminal and the stub is the straight segment from that cell to the pad
//! centre. The stub is not checked against the grid (it can come nearer a
//! neighbour than the clearance); KiCad DRC is the judge (M7.4).
//!
//! Plane nets (poured layers declared by the caller) connect by a via per pad
//! instead of by tracks, unless the pad is through-hole and so already
//! reaches the plane.

use std::collections::{BinaryHeap, HashMap};

use autolayout_board::{Board, Point};

use crate::model::{FREE, Lane, RoutingModel};

/// How far from a plane-net pad's centre (in pitches, beyond its own half
/// size) a tap via may sit; the stub that joins them is not grid-checked.
/// 1.5 left 4 of 104 m1 plane pads unreachable, 5 reaches all (2026-10-13).
pub const TAP_REACH_PITCHES: f64 = 5.0;

/// Present and history penalties stop growing here. Without a cap the
/// present penalty (x1.5 per iteration) overflowed `u32` near iteration 50 and
/// the search ran away until memory ran out (found 2026-10-13, m1 at 0.2 mm
/// rules, 60 iterations).
pub const PENALTY_CAP: u32 = 1_000_000;

/// How far from a pad's centre (in pitches, beyond its half-size) to look for
/// grid cells to land a stub on when no cell lies inside the pad. Two settings
/// are tried and the better result kept (`route` portfolio): the narrow one
/// leaves corridors free, the wide one reaches pads the narrow one cannot.
/// Neither dominates (local 37-board sweep, verifier not KiCad, unconnected
/// narrow 184 / 2.25 -> 207 / 3.0 -> 199 / wide 194, KiCad on the wide one 196
/// with four m1 returns 6-14 worse), so the choice is made per board.
pub const NEAR_REACH_NARROW: f64 = 1.5;
pub const NEAR_REACH_PITCHES: f64 = 4.5;

/// Bent stubs reach this many pitches beyond the pad end along its axis.
const BENT_AXIAL_PITCHES: f64 = 4.0;
/// ... and this many pitches to the side.
const BENT_LATERAL_PITCHES: f64 = 2.0;

/// Plane tap vias keep this far (mm) from the exit of another net's planned escape.
const TAP_SEED_CLEAR: f64 = 1.2;

/// Hard-mode owner of a cell two isolated stubs share: nobody may route there.
const SHARED: u32 = u32::MAX - 2;

/// Cost of one grid step in the layer's preferred direction.
pub const STEP: u32 = 10;

#[derive(Debug, Clone, Copy)]
pub struct Costs {
  pub step: u32,
  /// Step against the layer's preferred direction.
  pub wrong_way: u32,
  pub via: u32,
  /// How far, in pitches beyond a pad's half-size, a stub may reach for a
  /// grid cell when none lies inside the pad (see [`NEAR_REACH_PITCHES`]).
  pub reach: f64,
  /// Let stubs of different nets share corridor cells when they keep a legal
  /// distance (see [`NetRoute::stub_cells`]). On: adjacent fine-pitch pads
  /// can all escape. Off: the older, stricter rule, which wins on some boards
  /// (local 37-board sweep: 163 off, 152 on, 141 best of the two per board).
  pub share_stubs: bool,
  /// Pads with no straight-stub landing may use a bent stub (axis, then a 45 degree jog).
  pub bent_stubs: bool,
  /// Record, for every pad a net could not reach, why (`NetRoute::why`). Changes nothing else.
  pub explain: bool,
  /// Land each pad of a row of long pads on the grid line planned for it
  /// jointly with its neighbours (`RoutingModel::lanes`).
  pub lanes: bool,
}

impl Default for Costs {
  fn default() -> Costs {
    Costs {
      step: STEP,
      wrong_way: 25,
      via: 60,
      reach: NEAR_REACH_NARROW,
      share_stubs: true,
      bent_stubs: true,
      explain: false,
      lanes: false,
    }
  }
}

/// A straight run of cells on one layer (ix, iy at both ends).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
  pub layer: usize,
  pub a: (usize, usize),
  pub b: (usize, usize),
}

/// Copper from a cell centre to the pad's centre, for pads off the grid.
#[derive(Debug, Clone, PartialEq)]
pub struct Stub {
  pub layer: usize,
  pub from: Point,
  pub to: Point,
}

/// Why a pad stayed unreached, recorded only with `Costs::explain`. The search
/// grows from the pads already joined; when it fails, the end that reached
/// fewer grid states is the boxed-in one, which is not always the pad the
/// report names.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Why {
  /// The pad that stayed unreached.
  pub pad: (usize, usize),
  /// Pads already joined to the net when the search failed.
  pub tree_pads: Vec<(usize, usize)>,
  /// Grid states reached growing from the joined pads, and from this pad.
  pub tree_reach: usize,
  pub pad_reach: usize,
  /// What blocks the edge of each reached region, as (fixed obstacles,
  /// plane-net claims, other nets' claims) in neighbouring cells.
  pub tree_block: (usize, usize, usize),
  pub pad_block: (usize, usize, usize),
  /// Cell maps around the region each side reached (`o` reached, `#` fixed
  /// obstacle, `p` plane claim, `x` other net's claim, `.` free), per layer.
  pub tree_map: String,
  pub pad_map: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetRoute {
  pub net: u32,
  pub runs: Vec<Run>,
  pub vias: Vec<(usize, usize)>,
  pub stubs: Vec<Stub>,
  /// Every grid cell (layer, ix, iy) the paths pass through, sorted, no repeats.
  pub cells: Vec<(usize, usize, usize)>,
  /// Corridor cells of stubs that keep a legal distance from every other
  /// net's stub. Two such stubs may share a corridor cell (adjacent pads of a
  /// 0.4 mm-pitch part are 0.4 mm apart, less than two corridors wide), so
  /// these cells conflict with tracks but not with each other. Disjoint from
  /// `cells` after [`Router::footprint_s`].
  pub stub_cells: Vec<(usize, usize, usize)>,
  /// Pads (footprint index, pad index) joined into the net.
  pub connected: Vec<(usize, usize)>,
  /// Pads that could not be reached.
  pub failed: Vec<(usize, usize)>,
  /// Why each unreached pad was unreached (only with `Costs::explain`).
  pub why: Vec<Why>,
}

impl NetRoute {
  pub fn length_mm(&self, pitch: f64) -> f64 {
    self
      .runs
      .iter()
      .map(|r| (r.a.0.abs_diff(r.b.0) + r.a.1.abs_diff(r.b.1)) as f64 * pitch)
      .sum()
  }
}

/// Terminal of one pad: the states a path may end on.
#[derive(Clone)]
struct Terminal {
  pad: (usize, usize),
  cells: Vec<(usize, usize, usize)>,
  /// Cells reached by a bent stub (axis, then a 45 degree jog): the corner point.
  bends: HashMap<(usize, usize, usize), Point>,
  /// Cell box (min ix, max ix, min iy, max iy) for the heuristic.
  bbox: (usize, usize, usize, usize),
  /// The pad already has copper of its own net attached in the file and the
  /// cells are grid nodes that copper passes through: no stub is drawn.
  preconnected: bool,
}

/// State of negotiated congestion (M7.3): how many nets hold each cell now,
/// how often each cell has been fought over, and how hard sharing is punished.
pub struct Soft {
  pub occ: Vec<Vec<u16>>,
  /// Nets holding the cell only as an isolated stub corridor.
  pub occ_s: Vec<Vec<u16>>,
  pub hist: Vec<Vec<u32>>,
  pub pres: u32,
}

pub struct Router<'a> {
  pub model: &'a RoutingModel,
  pub costs: Costs,
  /// Which net holds each cell, per routable layer (net + 1), from routes
  /// already committed. Separate from the model's static obstacles.
  pub usage: Vec<Vec<u32>>,
  /// `Some` while negotiating: cells may be shared at a cost, and routes do
  /// not write `usage` (the driver keeps `soft.occ` instead).
  pub soft: Option<Soft>,
  // Search scratch, stamped so a search never clears the whole grid.
  stamp: Vec<u32>,
  cost: Vec<u64>,
  prev: Vec<u32>,
  epoch: u32,
  via_halo: Vec<(i64, i64)>,
  /// Cells too near a placed via's hole for another via (hole-to-hole rule).
  /// Fixed vias only (plane taps, and every via in greedy mode): while
  /// negotiating, vias of the net being routed are checked on their own.
  hole_block: Vec<bool>,
  spacing_offsets: Vec<(i64, i64)>,
  /// Via sites the current search must avoid (they broke the spacing rule
  /// against another via of the same path).
  banned: std::collections::HashSet<(usize, usize)>,
  /// Stubs of every net routed so far, to test a new stub's distance to them.
  stub_reg: std::collections::HashMap<u32, Vec<Stub>>,
  /// Stubs placed before negotiation began (plane taps): never re-routed, so
  /// they outlive [`Router::leave_soft`], which drops the negotiated ones.
  stub_fixed: std::collections::HashMap<u32, Vec<Stub>>,
  /// Hard mode: cells claimed only by an isolated stub (a track of the owner
  /// may use them; another net's isolated stub may share them).
  stub_only: Vec<Vec<bool>>,
}

impl<'a> Router<'a> {
  pub fn new(model: &'a RoutingModel, costs: Costs) -> Router<'a> {
    let n = model.nx * model.ny * model.layers.len();
    // A via claims every cell within via/2 + track/2 + clearance on every layer.
    let reach = model.profile.via_diameter / 2.0 + model.profile.track_width / 2.0 + model.profile.clearance;
    let r = (reach / model.pitch).ceil() as i64;
    let mut via_halo = Vec::new();
    for dy in -r..=r {
      for dx in -r..=r {
        if ((dx * dx + dy * dy) as f64).sqrt() * model.pitch <= reach {
          via_halo.push((dx, dy));
        }
      }
    }
    let sp = model.via_spacing;
    let sr = (sp / model.pitch).ceil() as i64;
    let mut spacing_offsets = Vec::new();
    for dy in -sr..=sr {
      for dx in -sr..=sr {
        if ((dx * dx + dy * dy) as f64).sqrt() * model.pitch < sp - 1e-9 {
          spacing_offsets.push((dx, dy));
        }
      }
    }
    Router {
      model,
      hole_block: vec![false; model.nx * model.ny],
      spacing_offsets,
      banned: std::collections::HashSet::new(),
      stub_reg: std::collections::HashMap::new(),
      stub_fixed: std::collections::HashMap::new(),
      stub_only: vec![vec![false; model.nx * model.ny]; model.layers.len()],
      costs,
      usage: vec![vec![FREE; model.nx * model.ny]; model.layers.len()],
      soft: None,
      stamp: vec![0; n],
      cost: vec![0; n],
      prev: vec![u32::MAX; n],
      epoch: 0,
      via_halo,
    }
  }

  fn state(&self, layer: usize, ix: usize, iy: usize) -> usize {
    (layer * self.model.ny + iy) * self.model.nx + ix
  }

  fn unstate(&self, s: usize) -> (usize, usize, usize) {
    let (nx, ny) = (self.model.nx, self.model.ny);
    (s / (nx * ny), s % nx, (s / nx) % ny)
  }

  /// Grid states the last search reached.
  fn reached_count(&self) -> usize {
    self.stamp.iter().filter(|&&s| s == self.epoch).count()
  }

  /// What blocks the edge of the region the last search reached: neighbouring
  /// cells that are fixed obstacles, claimed by a plane net, or claimed by
  /// another net.
  fn frontier_tally(&self, net: u32) -> (usize, usize, usize) {
    let m = self.model;
    let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
    let (mut fixed, mut plane, mut signal) = (0, 0, 0);
    let mut seen = std::collections::HashSet::new();
    for s in 0..self.stamp.len() {
      if self.stamp[s] != self.epoch {
        continue;
      }
      let (l, x, y) = self.unstate(s);
      for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
        let (xx, yy) = (x as i64 + dx, y as i64 + dy);
        if xx < 0 || yy < 0 || xx >= m.nx as i64 || yy >= m.ny as i64 {
          continue;
        }
        let (xx, yy) = (xx as usize, yy as usize);
        let ns = self.state(l, xx, yy);
        if self.stamp[ns] == self.epoch || !seen.insert(ns) {
          continue;
        }
        if !m.track_ok(l, xx, yy, net) {
          fixed += 1;
        } else if let Some(&o) = self.usage.get(l).and_then(|u| u.get(yy * m.nx + xx))
          && o != FREE
          && o != net + 1
        {
          if o != u32::MAX && plane_nets.contains(&(o - 1)) {
            plane += 1;
          } else {
            signal += 1;
          }
        }
      }
    }
    (fixed, plane, signal)
  }

  /// ASCII picture of the region the last search reached and its surroundings.
  fn reached_map(&self, net: u32) -> String {
    let m = self.model;
    let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0usize, usize::MAX, 0usize);
    for s in 0..self.stamp.len() {
      if self.stamp[s] == self.epoch {
        let (_, x, y) = self.unstate(s);
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
      }
    }
    if x0 > x1 {
      return String::new();
    }
    let (x1, y1) = (x1.min(x0 + 40), y1.min(y0 + 24));
    let (x0, y0) = (x0.saturating_sub(3), y0.saturating_sub(3));
    let (x1, y1) = (
      (x1 + 3).min(m.nx.saturating_sub(1)),
      (y1 + 3).min(m.ny.saturating_sub(1)),
    );
    let mut out = String::new();
    for l in 0..m.layers.len() {
      out.push_str(&format!(
        "    layer {} x {x0}..{x1} y {y0}..{y1}\n",
        m.layers.get(l).map_or("?", String::as_str)
      ));
      for y in y0..=y1 {
        out.push_str("    ");
        for x in x0..=x1 {
          let c = if self.stamp[self.state(l, x, y)] == self.epoch {
            'o'
          } else if !m.track_ok(l, x, y, net) {
            '#'
          } else {
            match self.usage.get(l).and_then(|u| u.get(y * m.nx + x)) {
              Some(&o) if o != FREE && o != net + 1 => {
                if o != u32::MAX && plane_nets.contains(&(o - 1)) {
                  'p'
                } else {
                  'x'
                }
              }
              _ => '.',
            }
          };
          out.push(c);
        }
        out.push('\n');
      }
    }
    out
  }

  fn cell_free(&self, layer: usize, ix: usize, iy: usize, net: u32) -> bool {
    self.model.track_ok(layer, ix, iy, net)
      && self
        .usage
        .get(layer)
        .and_then(|u| u.get(iy * self.model.nx + ix))
        .is_some_and(|&o| o == FREE || o == net + 1)
  }

  fn via_free(&self, ix: usize, iy: usize, net: u32) -> bool {
    if !self.model.via_ok(ix, iy, net) || self.banned.contains(&(ix, iy)) {
      return false;
    }
    if self
      .hole_block
      .get(iy * self.model.nx + ix)
      .copied()
      .unwrap_or(true)
    {
      return false;
    }
    for &(dx, dy) in &self.via_halo {
      let (x, y) = (ix as i64 + dx, iy as i64 + dy);
      if x < 0 || y < 0 || x >= self.model.nx as i64 || y >= self.model.ny as i64 {
        return false;
      }
      for u in &self.usage {
        match u.get(y as usize * self.model.nx + x as usize) {
          Some(&o) if o == FREE || o == net + 1 => {}
          _ => return false,
        }
      }
    }
    true
  }

  fn claim_cell(&mut self, layer: usize, ix: usize, iy: usize, net: u32) {
    if self.soft.is_some() {
      return;
    }
    let nx = self.model.nx;
    if let Some(c) = self.usage.get_mut(layer).and_then(|u| u.get_mut(iy * nx + ix)) {
      *c = net + 1;
    }
    if let Some(m) = self
      .stub_only
      .get_mut(layer)
      .and_then(|u| u.get_mut(iy * nx + ix))
    {
      *m = false;
    }
  }

  /// Hard mode: claim a cell for an isolated stub. A free cell becomes the
  /// stub's (and stays usable by its net's tracks); a cell another isolated
  /// stub already holds becomes SHARED: no track may use it, as one at its
  /// centre-line would sit too near both stubs.
  fn claim_stub_cell(&mut self, layer: usize, ix: usize, iy: usize, net: u32) {
    if self.soft.is_some() {
      return;
    }
    let i = iy * self.model.nx + ix;
    let owner = self.usage.get(layer).and_then(|u| u.get(i)).copied();
    let only = self
      .stub_only
      .get(layer)
      .and_then(|u| u.get(i))
      .copied()
      .unwrap_or(false);
    match owner {
      Some(o) if o == FREE => {
        if let Some(c) = self.usage.get_mut(layer).and_then(|u| u.get_mut(i)) {
          *c = net + 1;
        }
        if let Some(m) = self.stub_only.get_mut(layer).and_then(|u| u.get_mut(i)) {
          *m = true;
        }
      }
      Some(o) if o != net + 1 && only && o != SHARED => {
        if let Some(c) = self.usage.get_mut(layer).and_then(|u| u.get_mut(i)) {
          *c = SHARED;
        }
      }
      _ => {}
    }
  }

  /// Is the segment `a..b` on `layer` at least a pitch from every stub of any
  /// other net? A pitch is track + clearance + margin: the least that keeps
  /// two centre-lines legal.
  fn stub_isolated(&self, layer: usize, a: Point, b: Point, net: u32) -> bool {
    let need = self.model.pitch - 1e-9;
    self
      .stub_reg
      .iter()
      .chain(self.stub_fixed.iter())
      .all(|(&n, stubs)| {
        n == net
          || stubs.iter().all(|t| {
            t.layer != layer
              || (crate::shape::seg_dist(a, t.from, t.to)
                .min(crate::shape::seg_dist(b, t.from, t.to))
                .min(crate::shape::seg_dist(t.from, a, b))
                .min(crate::shape::seg_dist(t.to, a, b))
                >= need)
          })
      })
  }

  fn claim_via(&mut self, ix: usize, iy: usize, net: u32) {
    if self.soft.is_some() {
      return;
    }
    let (nx, ny) = (self.model.nx as i64, self.model.ny as i64);
    for &(dx, dy) in &self.via_halo.clone() {
      let (x, y) = (ix as i64 + dx, iy as i64 + dy);
      if x >= 0 && y >= 0 && x < nx && y < ny {
        for l in 0..self.usage.len() {
          self.claim_cell(l, x as usize, y as usize, net);
        }
      }
    }
    for &(dx, dy) in &self.spacing_offsets.clone() {
      let (x, y) = (ix as i64 + dx, iy as i64 + dy);
      if x >= 0
        && y >= 0
        && x < nx
        && y < ny
        && let Some(b) = self.hole_block.get_mut(y as usize * self.model.nx + x as usize)
      {
        *b = true;
      }
    }
  }

  /// Start negotiating. `present` is the first present-sharing penalty.
  pub fn enable_soft(&mut self, present: u32) {
    self.stub_fixed = self.stub_reg.clone();
    let (n, l) = (self.model.nx * self.model.ny, self.model.layers.len());
    self.soft = Some(Soft {
      occ: vec![vec![0; n]; l],
      occ_s: vec![vec![0; n]; l],
      hist: vec![vec![0; n]; l],
      pres: present,
    });
  }

  /// Added cost of putting a track on a cell: history plus the present
  /// penalty per other net already there.
  fn cell_extra(&self, layer: usize, ix: usize, iy: usize) -> u32 {
    let Some(s) = &self.soft else { return 0 };
    let i = iy * self.model.nx + ix;
    let h = s.hist.get(layer).and_then(|v| v.get(i)).copied().unwrap_or(0);
    let o = s.occ.get(layer).and_then(|v| v.get(i)).copied().unwrap_or(0);
    let o = o.saturating_add(s.occ_s.get(layer).and_then(|v| v.get(i)).copied().unwrap_or(0));
    h.saturating_add(s.pres.saturating_mul(u32::from(o)))
  }

  /// Added cost of a via: the same, summed over the cells its halo takes.
  fn via_extra(&self, ix: usize, iy: usize) -> u32 {
    let Some(s) = &self.soft else { return 0 };
    let mut sum: u32 = 0;
    for &(dx, dy) in &self.via_halo {
      let (x, y) = (ix as i64 + dx, iy as i64 + dy);
      if x < 0 || y < 0 || x >= self.model.nx as i64 || y >= self.model.ny as i64 {
        continue;
      }
      let i = y as usize * self.model.nx + x as usize;
      for l in 0..s.occ.len() {
        let o = s.occ.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0);
        let o = o.saturating_add(s.occ_s.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0));
        let hist = s.hist.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0) / 4;
        sum = sum
          .saturating_add(s.pres.saturating_mul(u32::from(o)))
          .saturating_add(hist);
      }
    }
    sum
  }

  /// Every (layer, cell index) the net's copper takes: its track cells and
  /// the halo of each via on every layer. Sorted, no repeats.
  pub fn footprint(&self, r: &NetRoute) -> Vec<(usize, usize)> {
    let (nx, ny) = (self.model.nx as i64, self.model.ny as i64);
    let mut v: Vec<(usize, usize)> = r
      .cells
      .iter()
      .map(|&(l, x, y)| (l, y * self.model.nx + x))
      .collect();
    for &(vx, vy) in &r.vias {
      for &(dx, dy) in &self.via_halo {
        let (x, y) = (vx as i64 + dx, vy as i64 + dy);
        if x >= 0 && y >= 0 && x < nx && y < ny {
          for l in 0..self.model.layers.len() {
            v.push((l, y as usize * self.model.nx + x as usize));
          }
        }
      }
    }
    v.sort_unstable();
    v.dedup();
    v
  }

  /// Cells the net holds only as an isolated stub corridor: not among its
  /// track cells ([`Router::footprint`]). Sorted, no repeats.
  pub fn footprint_s(&self, r: &NetRoute) -> Vec<(usize, usize)> {
    let t: std::collections::HashSet<(usize, usize)> = self.footprint(r).into_iter().collect();
    let mut v: Vec<(usize, usize)> = r
      .stub_cells
      .iter()
      .map(|&(l, x, y)| (l, y * self.model.nx + x))
      .filter(|c| !t.contains(c))
      .collect();
    v.sort_unstable();
    v.dedup();
    v
  }

  /// As [`Router::occupy`], for the stub-only cells.
  pub fn occupy_s(&mut self, fp: &[(usize, usize)], add: bool) {
    let Some(s) = &mut self.soft else { return };
    for &(l, i) in fp {
      if let Some(c) = s.occ_s.get_mut(l).and_then(|v| v.get_mut(i)) {
        *c = if add {
          c.saturating_add(1)
        } else {
          c.saturating_sub(1)
        };
      }
    }
  }

  /// Add (`+1`) or remove (`-1`) a net's footprint from the occupancy.
  pub fn occupy(&mut self, fp: &[(usize, usize)], add: bool) {
    let Some(s) = &mut self.soft else { return };
    for &(l, i) in fp {
      if let Some(c) = s.occ.get_mut(l).and_then(|v| v.get_mut(i)) {
        *c = if add {
          c.saturating_add(1)
        } else {
          c.saturating_sub(1)
        };
      }
    }
  }

  /// Cells held by more than one net.
  pub fn conflicts(&self) -> usize {
    self.soft.as_ref().map_or(0, |s| {
      s.occ
        .iter()
        .zip(&s.occ_s)
        .map(|(v, w)| {
          v.iter()
            .zip(w)
            .filter(|&(&o, &p)| o > 1 || (o >= 1 && p >= 1))
            .count()
        })
        .sum()
    })
  }

  /// Raise the history of every contested cell by `inc`.
  pub fn bump_history(&mut self, inc: u32) {
    if let Some(s) = &mut self.soft {
      for ((h, o), w) in s.hist.iter_mut().zip(&s.occ).zip(&s.occ_s) {
        for ((h, &o), &p) in h.iter_mut().zip(o).zip(w) {
          if o > 1 || (o >= 1 && p >= 1) {
            *h = h.saturating_add(inc).min(PENALTY_CAP);
          }
        }
      }
    }
  }

  /// Stop negotiating: from here routes are claimed in `usage` and cells are
  /// hard obstacles again (used to repair nets dropped at the end).
  pub fn leave_soft(&mut self) {
    self.soft = None;
    self.stub_reg.clear();
  }

  /// Claim a finished route's cells and via halos in `usage` as hard obstacles.
  /// Only meaningful after [`Router::leave_soft`].
  pub fn commit_hard(&mut self, r: &NetRoute) {
    for &(l, x, y) in &r.cells {
      self.claim_cell(l, x, y, r.net);
    }
    for &(l, x, y) in &r.stub_cells {
      self.claim_stub_cell(l, x, y, r.net);
    }
    for s in &r.stubs {
      self.stub_reg.entry(r.net).or_default().push(s.clone());
    }
    for &(x, y) in &r.vias {
      self.claim_via(x, y, r.net);
    }
  }

  /// Does a net's footprint (track cells `fp`, stub-only cells `fps`) share a
  /// cell it may not? Tracks conflict with tracks and with stubs; isolated
  /// stubs conflict only with tracks.
  pub fn is_contested(&self, fp: &[(usize, usize)], fps: &[(usize, usize)]) -> bool {
    self.soft.as_ref().is_some_and(|s| {
      let at = |a: &Vec<Vec<u16>>, l: usize, i: usize| a.get(l).and_then(|v| v.get(i)).copied().unwrap_or(0);
      fp.iter()
        .any(|&(l, i)| at(&s.occ, l, i) > 1 || at(&s.occ_s, l, i) >= 1)
        || fps.iter().any(|&(l, i)| at(&s.occ, l, i) >= 1)
    })
  }

  pub fn grow_present(&mut self, factor_num: u32, factor_den: u32) {
    if let Some(s) = &mut self.soft {
      s.pres = (s.pres.saturating_mul(factor_num) / factor_den.max(1))
        .max(s.pres.saturating_add(1))
        .min(PENALTY_CAP);
    }
  }

  /// Cells a path to this pad may end on.
  fn terminal(&self, board: &Board, fi: usize, pi: usize, net: u32) -> Option<Terminal> {
    board.footprints.get(fi)?.pads.get(pi)?;
    if let Some(seeds) = self.model.preconnected.get(&(fi, pi)) {
      let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
      for &(_, x, y) in seeds {
        (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
      }
      return Some(Terminal {
        pad: (fi, pi),
        cells: seeds.clone(),
        bends: HashMap::new(),
        bbox: (x0, x1, y0, y1),
        preconnected: true,
      });
    }
    let lane = if self.costs.lanes {
      self.model.lanes.get(&(fi, pi)).copied()
    } else {
      None
    };
    let on_lane =
      |x: usize, y: usize| lane.is_none_or(|l| if l.column { x == l.index } else { y == l.index });
    let mut inside = self.model.pad_access(board, fi, pi).cells;
    if lane.is_some() && inside.iter().any(|&(_, x, y)| on_lane(x, y)) {
      inside.retain(|&(_, x, y)| on_lane(x, y));
    }
    let mut bends = HashMap::new();
    let mut cells = if inside.is_empty() {
      self.near_cells(board, fi, pi, net, lane)
    } else {
      inside
    };
    if cells.is_empty() && self.costs.bent_stubs {
      for (l, x, y, corner) in self.bent_cells(board, fi, pi, net, lane) {
        cells.push((l, x, y));
        bends.insert((l, x, y), corner);
      }
    }
    if cells.is_empty() {
      return None;
    }
    let (mut x0, mut x1, mut y0, mut y1) = (usize::MAX, 0, usize::MAX, 0);
    for &(_, x, y) in &cells {
      (x0, x1, y0, y1) = (x0.min(x), x1.max(x), y0.min(y), y1.max(y));
    }
    Some(Terminal {
      pad: (fi, pi),
      cells,
      bends,
      bbox: (x0, x1, y0, y1),
      preconnected: false,
    })
  }

  /// Landing cells for a pad with no straight-stub landing: out along the
  /// pad's long axis, then a 45 degree jog onto a free grid cell. The corner
  /// is returned with each cell. Both segments must clear every foreign pad
  /// and claim only free corridor cells. At most four, shortest first.
  fn bent_cells(
    &self,
    board: &Board,
    fi: usize,
    pi: usize,
    net: u32,
    lane: Option<Lane>,
  ) -> Vec<(usize, usize, usize, Point)> {
    let Some(fp) = board.footprints.get(fi) else {
      return Vec::new();
    };
    let Some(pad) = fp.pads.get(pi) else {
      return Vec::new();
    };
    let (w, h) = crate::shape::pad_extent(pad);
    let (axis, half) = if w >= 1.5 * h {
      ((1.0, 0.0), w / 2.0)
    } else if h >= 1.5 * w {
      ((0.0, 1.0), h / 2.0)
    } else {
      return Vec::new();
    };
    // Outward is away from the middle of the part's pads; at the middle try both ways.
    let count = fp.pads.len().max(1) as f64;
    let (mx, my) = fp
      .pads
      .iter()
      .fold((0.0, 0.0), |(a, b), q| (a + q.at.x, b + q.at.y));
    let proj = (pad.at.x - mx / count) * axis.0 + (pad.at.y - my / count) * axis.1;
    let signs: Vec<f64> = if proj.abs() > 0.05 {
      vec![proj.signum()]
    } else {
      vec![1.0, -1.0]
    };
    let m = self.model;
    let layers = m.pad_layer_indices(&pad.layers);
    let radius = half + BENT_AXIAL_PITCHES * m.pitch;
    let lo = |v: f64, o: f64| ((v - o) / m.pitch).floor().max(0.0) as usize;
    let (x0, x1) = (
      lo(pad.at.x - radius, m.origin.x),
      lo(pad.at.x + radius, m.origin.x) + 1,
    );
    let (y0, y1) = (
      lo(pad.at.y - radius, m.origin.y),
      lo(pad.at.y + radius, m.origin.y) + 1,
    );
    let mut found: Vec<(u64, usize, usize, usize, Point)> = Vec::new();
    for sgn in signs {
      let u = (axis.0 * sgn, axis.1 * sgn);
      let nrm = (-u.1, u.0);
      for &l in &layers {
        for iy in y0..=y1.min(m.ny.saturating_sub(1)) {
          for ix in x0..=x1.min(m.nx.saturating_sub(1)) {
            if lane.is_some_and(|ln| if ln.column { ix != ln.index } else { iy != ln.index }) {
              continue;
            }
            let c = m.centre(ix, iy);
            let (dx, dy) = (c.x - pad.at.x, c.y - pad.at.y);
            let (a, lat) = (dx * u.0 + dy * u.1, dx * nrm.0 + dy * nrm.1);
            if lat.abs() > BENT_LATERAL_PITCHES * m.pitch || a - lat.abs() < half || a > radius {
              continue;
            }
            if !self.cell_free(l, ix, iy, net) {
              continue;
            }
            let k = a - lat.abs();
            let corner = Point::new(pad.at.x + u.0 * k, pad.at.y + u.1 * k);
            let end = self.stub_target(pad, corner);
            if self.seg_clear(board, fi, pi, c, corner, l, net)
              && self.seg_clear(board, fi, pi, corner, end, l, net)
              && self.corridor_free(l, c, corner, net)
              && self.corridor_free(l, corner, end, net)
            {
              let cost = (lat.abs() * std::f64::consts::SQRT_2 + k) * 1e6;
              found.push((cost as u64, l, ix, iy, corner));
            }
          }
        }
      }
    }
    found.sort_by(|a, b| {
      a.0
        .cmp(&b.0)
        .then(a.1.cmp(&b.1))
        .then(a.2.cmp(&b.2))
        .then(a.3.cmp(&b.3))
    });
    found.truncate(4);
    found.into_iter().map(|(_, l, x, y, c)| (l, x, y, c)).collect()
  }

  /// Usable cells within `NEAR_REACH_PITCHES` of the pad's centre, nearest four, on the
  /// pad's own copper layers.
  fn near_cells(
    &self,
    board: &Board,
    fi: usize,
    pi: usize,
    net: u32,
    lane: Option<Lane>,
  ) -> Vec<(usize, usize, usize)> {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return Vec::new();
    };
    let reach = self.model.pitch * self.costs.reach + pad.size.x.max(pad.size.y) / 2.0;
    let layers = self.model.pad_layer_indices(&pad.layers);
    let m = self.model;
    let lo = |v: f64, o: f64| ((v - o) / m.pitch).floor().max(0.0) as usize;
    let (x0, x1) = (
      lo(pad.at.x - reach, m.origin.x),
      lo(pad.at.x + reach, m.origin.x) + 1,
    );
    let (y0, y1) = (
      lo(pad.at.y - reach, m.origin.y),
      lo(pad.at.y + reach, m.origin.y) + 1,
    );
    let mut found: Vec<(u64, usize, usize, usize)> = Vec::new();
    for &l in &layers {
      for iy in y0..=y1.min(m.ny.saturating_sub(1)) {
        for ix in x0..=x1.min(m.nx.saturating_sub(1)) {
          let d = m.centre(ix, iy) - pad.at;
          if lane.is_some_and(|ln| if ln.column { ix != ln.index } else { iy != ln.index }) {
            continue;
          }
          if d.length() <= reach
            && self.cell_free(l, ix, iy, net)
            && self.stub_clear(board, fi, pi, m.centre(ix, iy), l, net)
            && self.corridor_free(l, m.centre(ix, iy), self.stub_target(pad, m.centre(ix, iy)), net)
          {
            found.push(((d.length() * 1e6) as u64, l, ix, iy));
          }
        }
      }
    }
    found.sort_unstable();
    found.truncate(4);
    found.into_iter().map(|(_, l, x, y)| (l, x, y)).collect()
  }

  /// Where a stub from `from` should end on `pad`: just inside the copper,
  /// half a track width plus 2 um from its edge, on the line to the pad centre.
  /// Ending at the centre made every stub as long as the pad's half-size
  /// longer than it needed to be, with a bigger corridor to keep clear (g2i
  /// went from 31 to 25 nets complete when stubs began claiming corridors).
  fn stub_target(&self, pad: &autolayout_board::Pad, from: Point) -> Point {
    let shape = crate::shape::pad_copper(pad);
    let need = -(self.model.profile.track_width / 2.0 + 0.002);
    let c = pad.at;
    if shape.sdf(c) > need || shape.sdf(from) <= need {
      return c; // too thin to inset, or already inside: aim at the centre
    }
    let at = |t: f64| Point::new(from.x + (c.x - from.x) * t, from.y + (c.y - from.y) * t);
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
      let mid = (lo + hi) / 2.0;
      if shape.sdf(at(mid)) <= need {
        hi = mid;
      } else {
        lo = mid;
      }
    }
    at(hi)
  }

  /// Grid cells (ix, iy) whose centre lies within one pitch of the segment
  /// `from..to`: where another net's centre-line would sit too near a stub of
  /// this profile (adjacent centre-lines are exactly one pitch apart). Cells
  /// are the right unit because every track and via halo is claimed in cells,
  /// so a stub that claims its corridor conflicts with them in the ordinary way.
  fn corridor(&self, from: Point, to: Point) -> Vec<(usize, usize)> {
    let m = self.model;
    let reach = m.pitch;
    let lo = |v: f64, o: f64| ((v - o) / m.pitch).floor().max(0.0) as usize;
    let (x0, x1) = (
      lo(from.x.min(to.x) - reach, m.origin.x),
      lo(from.x.max(to.x) + reach, m.origin.x) + 1,
    );
    let (y0, y1) = (
      lo(from.y.min(to.y) - reach, m.origin.y),
      lo(from.y.max(to.y) + reach, m.origin.y) + 1,
    );
    let mut out = Vec::new();
    for iy in y0..=y1.min(m.ny.saturating_sub(1)) {
      for ix in x0..=x1.min(m.nx.saturating_sub(1)) {
        if crate::shape::seg_dist(m.centre(ix, iy), from, to) < reach - 1e-9 {
          out.push((ix, iy));
        }
      }
    }
    out
  }

  /// No other net holds any cell of this stub's corridor on `layer` (hard
  /// `usage` only; while negotiating, conflicts are priced by occupancy).
  fn corridor_free(&self, layer: usize, from: Point, to: Point, net: u32) -> bool {
    let nx = self.model.nx;
    let iso = self.costs.share_stubs && self.stub_isolated(layer, from, to, net);
    self.corridor(from, to).iter().all(|&(ix, iy)| {
      let i = iy * nx + ix;
      self.usage.get(layer).and_then(|u| u.get(i)).is_some_and(|&o| {
        o == FREE
          || o == net + 1
          || (iso
            && o != SHARED
            && self
              .stub_only
              .get(layer)
              .and_then(|u| u.get(i))
              .copied()
              .unwrap_or(false))
      })
    })
  }

  /// Make a stub part of the net's footprint: its corridor cells are claimed
  /// (hard) and listed in the route's cells (so occupancy sees them).
  fn adopt_stub(&mut self, stub: Stub, net: u32, out: &mut NetRoute) {
    let iso = self.costs.share_stubs && self.stub_isolated(stub.layer, stub.from, stub.to, net);
    for (ix, iy) in self.corridor(stub.from, stub.to) {
      if iso {
        self.claim_stub_cell(stub.layer, ix, iy, net);
        out.stub_cells.push((stub.layer, ix, iy));
      } else {
        self.claim_cell(stub.layer, ix, iy, net);
        out.cells.push((stub.layer, ix, iy));
      }
    }
    self.stub_reg.entry(net).or_default().push(stub.clone());
    out.stubs.push(stub);
  }

  /// Is the straight stub from `from` to the centre of pad (`fi`, `pi`) clear
  /// of every foreign pad on `layer`? Stubs are drawn off the grid, so the grid
  /// cannot vouch for them: at 0.2 mm rules a plane-tap stub ran across the
  /// neighbouring pads of a 0.4 mm-pitch MCU (found by the verifier, 2026-10-13).
  /// Tracks of other nets are not consulted, only pads.
  fn stub_clear(&self, board: &Board, fi: usize, pi: usize, from: Point, layer: usize, net: u32) -> bool {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return false;
    };
    let end = self.stub_target(pad, from);
    self.seg_clear(board, fi, pi, from, end, layer, net)
  }

  /// As [`Router::stub_clear`] for the segment `from..end` of a stub of pad (`fi`, `pi`).
  #[allow(clippy::too_many_arguments)]
  fn seg_clear(
    &self,
    board: &Board,
    fi: usize,
    pi: usize,
    from: Point,
    end: Point,
    layer: usize,
    net: u32,
  ) -> bool {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return false;
    };
    let Some(layer_name) = self.model.layers.get(layer) else {
      return false;
    };
    let half = self.model.profile.track_width / 2.0;
    let need = self.model.profile.clearance + self.model.margin;
    for (qf, fp) in board.footprints.iter().enumerate() {
      for (qp, q) in fp.pads.iter().enumerate() {
        if (qf, qp) == (fi, pi)
          || q.net == Some(net)
          || q.kind == "np_thru_hole"
          || (q.at - pad.at).length() > 6.0
        {
          continue;
        }
        let on_layer = q.layers.iter().any(|l| {
          l == layer_name || l == "*.Cu" || (l == "F&B.Cu" && (layer_name == "F.Cu" || layer_name == "B.Cu"))
        });
        if on_layer
          && crate::shape::capsule_gap(from, end, half, &crate::shape::pad_copper(q))
            < need.max(q.clearance.map_or(0.0, |o| o + self.model.margin))
        {
          return false;
        }
      }
    }
    true
  }

  fn wrong_way(&self, layer: usize, dx: bool) -> bool {
    // Even layers prefer horizontal runs, odd ones vertical.
    layer.is_multiple_of(2) != dx
  }

  /// Route all pads of `net` (a list of (footprint, pad) on that net). Pads
  /// are joined nearest-first starting from the first. Claims the copper it
  /// finds in `self.usage`.
  pub fn route_net(&mut self, board: &Board, net: u32, pads: &[(usize, usize)]) -> NetRoute {
    let mut out = NetRoute {
      net,
      ..NetRoute::default()
    };
    self.stub_reg.remove(&net);
    let mut terms: Vec<Terminal> = Vec::new();
    for &(fi, pi) in pads {
      match self.terminal(board, fi, pi, net) {
        Some(t) => terms.push(t),
        None => out.failed.push((fi, pi)),
      }
    }
    if terms.is_empty() {
      return out;
    }
    let first = terms.remove(0);
    out.connected.push(first.pad);
    let mut tree: Vec<usize> = Vec::new();
    for &(l, x, y) in &first.cells {
      tree.push(self.state(l, x, y));
      self.claim_cell(l, x, y, net);
    }
    // Pads already in the tree; a stub is drawn for any off-grid cell a path leaves from.
    let mut joined: Vec<Terminal> = vec![first];

    let mut attempts = 0;
    while !terms.is_empty() {
      match self.search(net, &tree, &terms) {
        Some((path, _)) if attempts < 24 && self.spacing_violation(&path, &out.vias).is_some() => {
          // Two vias of one net too close: forbid the later site and search again.
          if let Some(site) = self.spacing_violation(&path, &out.vias) {
            self.banned.insert(site);
          }
          attempts += 1;
        }
        Some((path, ti)) => {
          self.banned.clear();
          attempts = 0;
          let term = terms.remove(ti);
          if let Some(&start) = path.first() {
            let cell = self.unstate(start);
            if let Some(t) = joined.iter().find(|t| t.cells.contains(&cell)) {
              self.stub_if_off_pad(board, t, cell, &mut out);
            }
          }
          self.commit_path(net, &path, &mut tree, &mut out);
          if let Some(&end) = path.last() {
            self.stub_if_off_pad(board, &term, self.unstate(end), &mut out);
          }
          out.connected.push(term.pad);
          for &(l, x, y) in &term.cells {
            tree.push(self.state(l, x, y));
          }
          joined.push(term);
        }
        None => {
          if self.costs.explain {
            let tree_reach = self.reached_count();
            let tree_block = self.frontier_tally(net);
            let tree_map = self.reached_map(net);
            let tree_pads: Vec<(usize, usize)> = joined.iter().map(|j| j.pad).collect();
            let rest: Vec<Terminal> = terms.clone();
            for t in &rest {
              // Grow from the unreached pad toward the joined ones to see its side.
              let from: Vec<usize> = t.cells.iter().map(|&(l, x, y)| self.state(l, x, y)).collect();
              let _ = self.search(net, &from, &joined);
              out.why.push(Why {
                pad: t.pad,
                tree_pads: tree_pads.clone(),
                tree_reach,
                pad_reach: self.reached_count(),
                tree_block,
                pad_block: self.frontier_tally(net),
                tree_map: tree_map.clone(),
                pad_map: self.reached_map(net),
              });
            }
          }
          self.banned.clear();
          for t in terms.drain(..) {
            out.failed.push(t.pad);
          }
        }
      }
    }
    tree.sort_unstable();
    out.cells.sort_unstable();
    out.cells.dedup();
    out
  }

  /// The first via in `path` that sits nearer than `via_spacing` to another
  /// via already committed by this net or earlier in the same path.
  fn spacing_violation(&self, path: &[usize], committed: &[(usize, usize)]) -> Option<(usize, usize)> {
    let mut sites: Vec<(usize, usize)> = Vec::new();
    for w in path.windows(2) {
      if let [a, b] = w {
        let (la, xa, ya) = self.unstate(*a);
        let (lb, xb, yb) = self.unstate(*b);
        if (xa, ya) == (xb, yb) && la != lb && !sites.contains(&(xa, ya)) {
          sites.push((xa, ya));
        }
      }
    }
    let mut seen: Vec<(usize, usize)> = committed.to_vec();
    for site in sites {
      if !seen.contains(&site) {
        let c = self.model.centre(site.0, site.1);
        let too_near = seen
          .iter()
          .any(|&(ox, oy)| (self.model.centre(ox, oy) - c).length() < self.model.via_spacing - 1e-9);
        if too_near {
          return Some(site);
        }
      }
      seen.push(site);
    }
    None
  }

  fn stub_if_off_pad(
    &mut self,
    board: &Board,
    t: &Terminal,
    cell: (usize, usize, usize),
    out: &mut NetRoute,
  ) {
    if t.preconnected {
      return;
    }
    let (l, x, y) = cell;
    let Some(pad) = board.footprints.get(t.pad.0).and_then(|f| f.pads.get(t.pad.1)) else {
      return;
    };
    let p = self.model.centre(x, y);
    // Inside the pad's copper when the cell is one of the pad-access cells.
    let inside = self
      .model
      .pad_access(board, t.pad.0, t.pad.1)
      .cells
      .contains(&cell);
    if let Some(&corner) = t.bends.get(&cell) {
      if !inside {
        let first = Stub {
          layer: l,
          from: p,
          to: corner,
        };
        let second = Stub {
          layer: l,
          from: corner,
          to: self.stub_target(pad, corner),
        };
        for stub in [first, second] {
          if !out.stubs.contains(&stub) {
            self.adopt_stub(stub, out.net, out);
          }
        }
      }
      return;
    }
    let stub = Stub {
      layer: l,
      from: p,
      to: self.stub_target(pad, p),
    };
    if !inside && !out.stubs.contains(&stub) {
      self.adopt_stub(stub, out.net, out);
    }
  }

  fn commit_path(&mut self, net: u32, path: &[usize], tree: &mut Vec<usize>, out: &mut NetRoute) {
    let cells: Vec<(usize, usize, usize)> = path.iter().map(|&s| self.unstate(s)).collect();
    for w in cells.windows(2) {
      if let [a, b] = w
        && a.1 == b.1
        && a.2 == b.2
        && a.0 != b.0
        && !out.vias.contains(&(a.1, a.2))
      {
        out.vias.push((a.1, a.2));
        self.claim_via(a.1, a.2, net);
        for l in 0..self.model.layers.len() {
          tree.push(self.state(l, a.1, a.2));
        }
      }
    }
    for &(l, x, y) in &cells {
      self.claim_cell(l, x, y, net);
      tree.push(self.state(l, x, y));
      out.cells.push((l, x, y));
    }
    // Merge consecutive same-layer cells in a line into runs.
    let mut i = 0;
    while i < cells.len() {
      let Some(&(l, x0, y0)) = cells.get(i) else { break };
      let mut j = i;
      while let (Some(&(l2, x2, y2)), Some(&(_, x1, y1))) = (cells.get(j + 1), cells.get(j)) {
        let straight = l2 == l && (x2 == x1 || y2 == y1) && (x2 == x0 || y2 == y0);
        if straight && (x2, y2) != (x1, y1) {
          j += 1;
        } else {
          break;
        }
      }
      if j > i
        && let Some(&(_, x1, y1)) = cells.get(j)
      {
        out.runs.push(Run {
          layer: l,
          a: (x0, y0),
          b: (x1, y1),
        });
      }
      i = j.max(i + 1);
    }
    tree.sort_unstable();
    tree.dedup();
  }

  /// A* from every tree state to the cheapest cell of any remaining terminal.
  /// Returns the state path (source first) and the index of the terminal hit.
  fn search(&mut self, net: u32, tree: &[usize], terms: &[Terminal]) -> Option<(Vec<usize>, usize)> {
    self.epoch = self.epoch.wrapping_add(1);
    if self.epoch == 0 {
      self.stamp.iter_mut().for_each(|s| *s = 0);
      self.epoch = 1;
    }
    let mut goal: HashMap<usize, usize> = HashMap::new();
    for (ti, t) in terms.iter().enumerate() {
      for &(l, x, y) in &t.cells {
        goal.entry(self.state(l, x, y)).or_insert(ti);
      }
    }
    let h = |x: usize, y: usize| -> u64 {
      let mut best = u32::MAX;
      for t in terms {
        let (x0, x1, y0, y1) = t.bbox;
        let dx = x0.saturating_sub(x).max(x.saturating_sub(x1));
        let dy = y0.saturating_sub(y).max(y.saturating_sub(y1));
        best = best.min((dx + dy) as u32);
      }
      u64::from(best) * u64::from(STEP)
    };

    // Heap entries: (Reverse f, Reverse state) → smallest f, then smallest state.
    let mut heap: BinaryHeap<(std::cmp::Reverse<u64>, std::cmp::Reverse<u32>)> = BinaryHeap::new();
    for &s in tree {
      let (_, x, y) = self.unstate(s);
      self.stamp[s] = self.epoch;
      self.cost[s] = 0;
      self.prev[s] = u32::MAX;
      heap.push((std::cmp::Reverse(h(x, y)), std::cmp::Reverse(s as u32)));
    }
    let (nx, ny, nl) = (self.model.nx, self.model.ny, self.model.layers.len());
    while let Some((std::cmp::Reverse(f), std::cmp::Reverse(s32))) = heap.pop() {
      let s = s32 as usize;
      let g = self.cost[s];
      let (l, x, y) = self.unstate(s);
      if f > g + h(x, y) {
        continue; // a cheaper way to this state was found after this entry
      }
      if let Some(&ti) = goal.get(&s)
        && !tree.contains(&s)
      {
        let mut path = vec![s];
        let mut cur = s;
        while let Some(&p) = self.prev.get(cur) {
          if p == u32::MAX {
            break;
          }
          path.push(p as usize);
          cur = p as usize;
        }
        path.reverse();
        return Some((path, ti));
      }
      let mut relax = |me: &mut Self, ns: usize, step: u32, nxx: usize, nyy: usize| {
        let ng = g + u64::from(step);
        if me.stamp[ns] != me.epoch || ng < me.cost[ns] {
          me.stamp[ns] = me.epoch;
          me.cost[ns] = ng;
          me.prev[ns] = s as u32;
          heap.push((std::cmp::Reverse(ng + h(nxx, nyy)), std::cmp::Reverse(ns as u32)));
        }
      };
      for (dx, dy) in [(1i64, 0i64), (-1, 0), (0, 1), (0, -1)] {
        let (xx, yy) = (x as i64 + dx, y as i64 + dy);
        if xx < 0 || yy < 0 || xx >= nx as i64 || yy >= ny as i64 {
          continue;
        }
        let (xx, yy) = (xx as usize, yy as usize);
        if !self.cell_free(l, xx, yy, net) {
          continue;
        }
        let step = if self.wrong_way(l, dx != 0) {
          self.costs.wrong_way
        } else {
          self.costs.step
        };
        let ns = self.state(l, xx, yy);
        let extra = self.cell_extra(l, xx, yy);
        relax(self, ns, step.saturating_add(extra), xx, yy);
      }
      if nl > 1 && self.via_free(x, y, net) {
        for l2 in 0..nl {
          if l2 != l && self.cell_free(l2, x, y, net) {
            let ns = self.state(l2, x, y);
            let extra = self.via_extra(x, y).saturating_add(self.cell_extra(l2, x, y));
            relax(self, ns, self.costs.via.saturating_add(extra), x, y);
          }
        }
      }
    }
    None
  }

  /// Is `p` within `TAP_SEED_CLEAR` of a planned escape's exit of another net?
  /// A plane tap via there would claim the cells that exit needs.
  fn near_foreign_seed(&self, p: Point, net: u32) -> bool {
    self
      .model
      .seed_points
      .iter()
      .any(|&(q, n)| n != net && (q - p).length() < TAP_SEED_CLEAR)
  }

  /// Connect a plane-net pad to its plane by a via at the nearest usable cell.
  /// Through-hole pads that reach the plane layer already are, and need none.
  pub fn tap_plane(&mut self, board: &Board, fi: usize, pi: usize, net: u32) -> Option<NetRoute> {
    let pad = board.footprints.get(fi)?.pads.get(pi)?;
    let mut out = NetRoute {
      net,
      ..NetRoute::default()
    };
    let on_plane = self
      .model
      .planes
      .iter()
      .any(|p| p.net == net && pad.layers.iter().any(|l| l == "*.Cu" || l == &p.layer));
    // Copper of this net already attached to the pad includes a via: a through
    // via reaches the plane, so the pad is tapped.
    if on_plane || self.model.preconnected_via.contains(&(fi, pi)) {
      out.connected.push((fi, pi));
      return Some(out);
    }
    let m = self.model;
    let reach = m.pitch * TAP_REACH_PITCHES + pad.size.x.max(pad.size.y) / 2.0;
    let layers = m.pad_layer_indices(&pad.layers);
    let lo = |v: f64, o: f64| ((v - o) / m.pitch).floor().max(0.0) as usize;
    let (x0, x1) = (
      lo(pad.at.x - reach, m.origin.x),
      lo(pad.at.x + reach, m.origin.x) + 1,
    );
    let (y0, y1) = (
      lo(pad.at.y - reach, m.origin.y),
      lo(pad.at.y + reach, m.origin.y) + 1,
    );
    let on_pad: Vec<(usize, usize, usize)> = m.pad_access(board, fi, pi).cells;
    let mut best: Option<(u64, usize, usize, usize)> = None;
    for &l in &layers {
      for iy in y0..=y1.min(m.ny.saturating_sub(1)) {
        for ix in x0..=x1.min(m.nx.saturating_sub(1)) {
          let d = (m.centre(ix, iy) - pad.at).length();
          let key = (d * 1e6) as u64;
          if d <= reach
            && best.is_none_or(|b| key < b.0)
            && self.cell_free(l, ix, iy, net)
            && self.via_free(ix, iy, net)
            && !self.near_foreign_seed(m.centre(ix, iy), net)
            && (on_pad.contains(&(l, ix, iy))
              || (self.stub_clear(board, fi, pi, m.centre(ix, iy), l, net)
                && self.corridor_free(l, m.centre(ix, iy), self.stub_target(pad, m.centre(ix, iy)), net)))
          {
            best = Some((key, l, ix, iy));
          }
        }
      }
    }
    let Some((_, l, ix, iy)) = best else {
      out.failed.push((fi, pi));
      return Some(out);
    };
    self.claim_via(ix, iy, net);
    out.vias.push((ix, iy));
    if m
      .pad_access(board, fi, pi)
      .cells
      .iter()
      .all(|&c| c != (l, ix, iy))
    {
      self.adopt_stub(
        Stub {
          layer: l,
          from: m.centre(ix, iy),
          to: self.stub_target(pad, m.centre(ix, iy)),
        },
        net,
        &mut out,
      );
    }
    out.connected.push((fi, pi));
    Some(out)
  }
}
