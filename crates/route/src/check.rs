//! Self-consistency of a routing result against its own model. **Not a
//! grade** (CLAUDE.md rule 3): it re-reads the routes, not a written file,
//! with the model's own geometry. It catches bugs in the search; whether the
//! copper is legal is KiCad DRC's call (M7.4).
//!
//! Checks: (1) every track cell is one its net may use; (2) no cell on a
//! layer belongs to two nets; (3) every pad listed as connected is in one
//! connected component with the net's first pad, by cells, vias and stubs.

use std::collections::{HashMap, HashSet};

use autolayout_board::Board;

use crate::model::RoutingModel;
use crate::search::NetRoute;

#[derive(Debug, Default, PartialEq)]
pub struct CheckReport {
  pub illegal_cells: usize,
  pub shared_cells: usize,
  pub disconnected_pads: usize,
  pub nets_checked: usize,
}

impl CheckReport {
  pub fn clean(&self) -> bool {
    self.illegal_cells == 0 && self.shared_cells == 0 && self.disconnected_pads == 0
  }
}

fn expand(r: &NetRoute) -> HashSet<(usize, usize, usize)> {
  let mut cells = HashSet::new();
  for run in &r.runs {
    let (x0, x1) = (run.a.0.min(run.b.0), run.a.0.max(run.b.0));
    let (y0, y1) = (run.a.1.min(run.b.1), run.a.1.max(run.b.1));
    for y in y0..=y1 {
      for x in x0..=x1 {
        cells.insert((run.layer, x, y));
      }
    }
  }
  cells
}

pub fn check(board: &Board, model: &RoutingModel, routes: &[NetRoute]) -> CheckReport {
  let mut rep = CheckReport::default();
  let mut owner: HashMap<(usize, usize, usize), u32> = HashMap::new();
  for r in routes {
    let cells = expand(r);
    for &(l, x, y) in &cells {
      if !model.track_ok(l, x, y, r.net) {
        rep.illegal_cells += 1;
      }
      if let Some(&o) = owner.get(&(l, x, y))
        && o != r.net
      {
        rep.shared_cells += 1;
      }
      owner.insert((l, x, y), r.net);
    }
    // Plane nets join through the plane, not through copper we drew.
    if model.planes.iter().any(|p| p.net == r.net) {
      rep.nets_checked += 1;
      continue;
    }
    rep.nets_checked += 1;
    if r.connected.len() < 2 {
      continue;
    }
    // Union-find over: run cells (neighbours along a run), via columns, and
    // one node per pad joined to every cell inside its copper and to the
    // cell a stub leaves from.
    let mut uf = UnionFind::default();
    let mut sorted: Vec<_> = cells.iter().copied().collect();
    sorted.sort_unstable();
    for &(l, x, y) in &sorted {
      for n in [(l, x + 1, y), (l, x, y + 1)] {
        if cells.contains(&n) && joined(r, (l, x, y), n) {
          uf.union((l, x, y), n);
        }
      }
    }
    for &(x, y) in &r.vias {
      for l in 1..model.layers.len() {
        uf.union((0, x, y), (l, x, y));
      }
    }
    let mut pad_roots = Vec::new();
    for (k, &(fi, pi)) in r.connected.iter().enumerate() {
      let pad_node = (usize::MAX, k, 0);
      uf.add(pad_node);
      let mut tied = false;
      for c in model.pad_access(board, fi, pi).cells {
        if uf.has(c) {
          uf.union(pad_node, c);
          tied = true;
        }
      }
      let at = board
        .footprints
        .get(fi)
        .and_then(|f| f.pads.get(pi))
        .map(|p| p.at);
      for s in r.stubs.iter().filter(|s| Some(s.to) == at) {
        let ix = ((s.from.x - model.origin.x) / model.pitch).round() as usize;
        let iy = ((s.from.y - model.origin.y) / model.pitch).round() as usize;
        if uf.has((s.layer, ix, iy)) {
          uf.union(pad_node, (s.layer, ix, iy));
          tied = true;
        }
      }
      if !tied {
        rep.disconnected_pads += 1;
      }
      pad_roots.push(pad_node);
    }
    let mut roots: Vec<usize> = pad_roots.iter().map(|&n| uf.root(n)).collect();
    roots.sort_unstable();
    roots.dedup();
    rep.disconnected_pads += roots.len().saturating_sub(1);
  }
  rep
}

type Cell = (usize, usize, usize);

#[derive(Default)]
struct UnionFind {
  ids: HashMap<Cell, usize>,
  parent: Vec<usize>,
}

impl UnionFind {
  fn add(&mut self, c: Cell) -> usize {
    if let Some(&i) = self.ids.get(&c) {
      return i;
    }
    let i = self.parent.len();
    self.parent.push(i);
    self.ids.insert(c, i);
    i
  }

  fn has(&self, c: Cell) -> bool {
    self.ids.contains_key(&c)
  }

  fn find(&mut self, mut x: usize) -> usize {
    while let Some(&p) = self.parent.get(x) {
      if p == x {
        break;
      }
      let gp = self.parent.get(p).copied().unwrap_or(p);
      if let Some(e) = self.parent.get_mut(x) {
        *e = gp;
      }
      x = gp;
    }
    x
  }

  fn root(&mut self, c: Cell) -> usize {
    let i = self.add(c);
    self.find(i)
  }

  fn union(&mut self, a: Cell, b: Cell) {
    let (ra, rb) = (self.root(a), self.root(b));
    if let Some(e) = self.parent.get_mut(ra) {
      *e = rb;
    }
  }
}

/// Are two adjacent cells on the same run (so wire runs between them)?
fn joined(r: &NetRoute, a: (usize, usize, usize), b: (usize, usize, usize)) -> bool {
  if a.1 == b.1 && a.2 == b.2 {
    return a.0 != b.0 && r.vias.contains(&(a.1, a.2));
  }
  r.runs.iter().any(|run| {
    run.layer == a.0
      && a.0 == b.0
      && [a, b].iter().all(|c| {
        c.1 >= run.a.0.min(run.b.0)
          && c.1 <= run.a.0.max(run.b.0)
          && c.2 >= run.a.1.min(run.b.1)
          && c.2 <= run.a.1.max(run.b.1)
      })
  })
}
