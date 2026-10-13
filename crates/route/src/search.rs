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

use crate::model::{FREE, RoutingModel};

/// How far from a plane-net pad's centre (in pitches, beyond its own half
/// size) a tap via may sit; the stub that joins them is not grid-checked.
/// 1.5 left 4 of 104 m1 plane pads unreachable, 5 reaches all (2026-10-13).
pub const TAP_REACH_PITCHES: f64 = 5.0;

/// Present and history penalties stop growing here. Without a cap the
/// present penalty (x1.5 per iteration) overflowed `u32` near iteration 50 and
/// the search ran away until memory ran out (found 2026-10-13, m1 at 0.2 mm
/// rules, 60 iterations).
pub const PENALTY_CAP: u32 = 1_000_000;

/// Cost of one grid step in the layer's preferred direction.
pub const STEP: u32 = 10;

#[derive(Debug, Clone, Copy)]
pub struct Costs {
  pub step: u32,
  /// Step against the layer's preferred direction.
  pub wrong_way: u32,
  pub via: u32,
}

impl Default for Costs {
  fn default() -> Costs {
    Costs {
      step: STEP,
      wrong_way: 25,
      via: 60,
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NetRoute {
  pub net: u32,
  pub runs: Vec<Run>,
  pub vias: Vec<(usize, usize)>,
  pub stubs: Vec<Stub>,
  /// Every grid cell (layer, ix, iy) the paths pass through, sorted, no repeats.
  pub cells: Vec<(usize, usize, usize)>,
  /// Pads (footprint index, pad index) joined into the net.
  pub connected: Vec<(usize, usize)>,
  /// Pads that could not be reached.
  pub failed: Vec<(usize, usize)>,
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
struct Terminal {
  pad: (usize, usize),
  cells: Vec<(usize, usize, usize)>,
  /// Cell box (min ix, max ix, min iy, max iy) for the heuristic.
  bbox: (usize, usize, usize, usize),
}

/// State of negotiated congestion (M7.3): how many nets hold each cell now,
/// how often each cell has been fought over, and how hard sharing is punished.
pub struct Soft {
  pub occ: Vec<Vec<u16>>,
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
    let (n, l) = (self.model.nx * self.model.ny, self.model.layers.len());
    self.soft = Some(Soft {
      occ: vec![vec![0; n]; l],
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
      s.occ.iter().map(|v| v.iter().filter(|&&o| o > 1).count()).sum()
    })
  }

  /// Raise the history of every contested cell by `inc`.
  pub fn bump_history(&mut self, inc: u32) {
    if let Some(s) = &mut self.soft {
      for (h, o) in s.hist.iter_mut().zip(&s.occ) {
        for (h, &o) in h.iter_mut().zip(o) {
          if o > 1 {
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
  }

  /// Claim a finished route's cells and via halos in `usage` as hard obstacles.
  /// Only meaningful after [`Router::leave_soft`].
  pub fn commit_hard(&mut self, r: &NetRoute) {
    for &(l, x, y) in &r.cells {
      self.claim_cell(l, x, y, r.net);
    }
    for &(x, y) in &r.vias {
      self.claim_via(x, y, r.net);
    }
  }

  pub fn is_contested(&self, fp: &[(usize, usize)]) -> bool {
    self.soft.as_ref().is_some_and(|s| {
      fp.iter()
        .any(|&(l, i)| s.occ.get(l).and_then(|v| v.get(i)).is_some_and(|&o| o > 1))
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
    let inside = self.model.pad_access(board, fi, pi).cells;
    let cells = if inside.is_empty() {
      self.near_cells(board, fi, pi, net)
    } else {
      inside
    };
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
      bbox: (x0, x1, y0, y1),
    })
  }

  /// Usable cells within 1.5 pitch of the pad's centre, nearest four, on the
  /// pad's own copper layers.
  fn near_cells(&self, board: &Board, fi: usize, pi: usize, net: u32) -> Vec<(usize, usize, usize)> {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return Vec::new();
    };
    let reach = self.model.pitch * 1.5 + pad.size.x.max(pad.size.y) / 2.0;
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
          if d.length() <= reach
            && self.cell_free(l, ix, iy, net)
            && self.stub_clear(board, fi, pi, m.centre(ix, iy), l, net)
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

  /// Is the straight stub from `from` to the centre of pad (`fi`, `pi`) clear
  /// of every foreign pad on `layer`? Stubs are drawn off the grid, so the grid
  /// cannot vouch for them: at 0.2 mm rules a plane-tap stub ran across the
  /// neighbouring pads of a 0.4 mm-pitch MCU (found by the verifier, 2026-10-13).
  /// Tracks of other nets are not consulted, only pads.
  fn stub_clear(&self, board: &Board, fi: usize, pi: usize, from: Point, layer: usize, net: u32) -> bool {
    let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
      return false;
    };
    let Some(layer_name) = self.model.layers.get(layer) else {
      return false;
    };
    let half = self.model.profile.track_width / 2.0;
    let need = self.model.profile.clearance;
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
          && crate::shape::capsule_gap(from, pad.at, half, &crate::shape::pad_copper(q))
            < need.max(q.clearance.unwrap_or(0.0))
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

  fn stub_if_off_pad(&self, board: &Board, t: &Terminal, cell: (usize, usize, usize), out: &mut NetRoute) {
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
    let stub = Stub {
      layer: l,
      from: p,
      to: pad.at,
    };
    if !inside && !out.stubs.contains(&stub) {
      out.stubs.push(stub);
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
    if on_plane {
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
            && (on_pad.contains(&(l, ix, iy)) || self.stub_clear(board, fi, pi, m.centre(ix, iy), l, net))
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
      out.stubs.push(Stub {
        layer: l,
        from: m.centre(ix, iy),
        to: pad.at,
      });
    }
    out.connected.push((fi, pi));
    Some(out)
  }
}
