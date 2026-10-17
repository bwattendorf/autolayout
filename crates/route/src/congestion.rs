//! Layer-count prediction by tile-level global routing (M7.5, 2026-10-16).
//!
//! Is a board routable on `L` signal layers? This module answers with
//! arithmetic on a coarse graph, before any track is laid.
//!
//! **Graph.** The board is cut into square tiles (default 1.5 mm, about five
//! track pitches). Every tile exists on every layer. A tile edge's capacity on a
//! layer is the number of tracks that fit across its free stretches, by the
//! capacity formula (`capacity.rs`): a centre-line keeps `track/2 + clearance`
//! from any copper edge on that layer, two centre-lines `track + clearance`
//! apart, so a free stretch of length `L` holds `floor(L / (track + clearance)) + 1`.
//! Layers beyond the board's own are empty except for through-hole pads (which
//! reach every copper layer).
//!
//! **Vias.** A layer change is a via, and a via takes room on every layer: a
//! tile's via capacity is its via-legal free area divided by the area one via
//! takes in a hexagonal packing (`0.866 * spacing^2`, spacing = the least
//! via-to-via distance). Every layer change in a net's route spends one unit in
//! that tile; a plane-net pad spends one unit in its tile (it needs a via to
//! its plane, not a track).
//!
//! **Demand.** Every other net is routed as a tree on the graph (pads nearest
//! first), PathFinder style: all nets are ripped up and re-routed in turn,
//! paying for edges and via tiles that are over capacity and for ones that have
//! been, until nothing is over capacity or the iterations run out.
//!
//! **Result.** The overflow left (tracks over edge capacity plus vias over tile
//! capacity) per layer count; zero overflow means the layer count is enough *as
//! far as this model sees*, positive overflow after negotiation means more room
//! or more layers are needed there. It does not see pad access inside a tile
//! (a pad boxed in by its neighbours), tracks crossing a tile edge obliquely or
//! a tile's internal obstacles, so zero overflow is necessary, not sufficient.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use autolayout_board::{BBox, Board, Point};

use crate::capacity::{CapacityRules, on_layer};
use crate::shape::{Shape, pad_copper};

/// Settings of one prediction.
#[derive(Debug, Clone, Copy)]
pub struct CongestionParams {
  /// Tile side, mm.
  pub tile: f64,
  pub iters: usize,
  /// Cost of a layer change, in tile steps.
  pub via_cost: f32,
}

impl Default for CongestionParams {
  fn default() -> CongestionParams {
    CongestionParams {
      tile: 1.5,
      iters: 40,
      via_cost: 2.0,
    }
  }
}

/// One over-capacity place.
#[derive(Debug, Clone, PartialEq)]
pub struct Hotspot {
  /// Tile centre.
  pub at: Point,
  pub layer: Option<usize>,
  pub what: &'static str,
  pub usage: u32,
  pub capacity: u32,
}

/// Outcome for one layer count.
#[derive(Debug, Clone, Default)]
pub struct Prediction {
  pub layers: usize,
  pub overflow: u32,
  pub overflow_edges: usize,
  pub overflow_via_tiles: usize,
  /// Tracks over capacity, summed, per layer.
  pub per_layer_overflow: Vec<u32>,
  /// Layer changes the routes use, and the via capacity of the board.
  pub vias_used: u32,
  pub via_capacity: u32,
  pub hotspots: Vec<Hotspot>,
  pub nets: usize,
  pub iterations: usize,
  pub tiles: (usize, usize),
}

/// Tracks that fit in the free stretches of an open run of samples `step` apart.
fn tracks_in_runs(free: &[bool], step: f64, pitch: f64) -> u32 {
  let mut total = 0u32;
  let mut len: Option<usize> = None;
  for &f in free.iter().chain(std::iter::once(&false)) {
    match (f, len) {
      (true, None) => len = Some(1),
      (true, Some(l)) => len = Some(l + 1),
      (false, Some(l)) => {
        total += (((l - 1) as f64 * step) / pitch + 1e-9).floor() as u32 + 1;
        len = None;
      }
      (false, None) => {}
    }
  }
  total
}

struct PadObs {
  shape: Shape,
  layers: Vec<String>,
  centre: Point,
  reach: f64,
}

/// Capacity of every tile edge on every layer, and via capacity of every tile.
struct Graph {
  nx: usize,
  ny: usize,
  nl: usize,
  /// (edge capacity, indexed `(l * tiles + tile) * 2 + dir`; dir 0 = to the right, 1 = down).
  cap_edge: Vec<u32>,
  cap_via: Vec<u32>,
  x0: f64,
  y0: f64,
  tile: f64,
}

impl Graph {
  fn tiles(&self) -> usize {
    self.nx * self.ny
  }

  fn centre(&self, t: usize) -> Point {
    Point::new(
      self.x0 + ((t % self.nx) as f64 + 0.5) * self.tile,
      self.y0 + ((t / self.nx) as f64 + 0.5) * self.tile,
    )
  }
}

fn build_graph(
  board: &Board,
  rules: &CapacityRules,
  via: (f64, f64),
  layer_names: &[String],
  outline: BBox,
  tile: f64,
) -> Graph {
  let (via_dia, via_spacing) = via;
  let nx = ((outline.width() / tile).ceil() as usize).max(1);
  let ny = ((outline.height() / tile).ceil() as usize).max(1);
  let nl = layer_names.len();
  let (x0, y0) = (outline.min.x, outline.min.y);
  // Pads, bucketed by 2 mm cells for the distance checks.
  let pads: Vec<PadObs> = board
    .footprints
    .iter()
    .flat_map(|f| f.pads.iter())
    .filter(|p| p.kind != "np_thru_hole" || p.drill.is_some())
    .filter_map(|p| {
      let shape = pad_copper(p);
      let bb = shape.bbox()?;
      Some(PadObs {
        reach: bb.width().hypot(bb.height()) / 2.0,
        centre: Point::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0),
        shape,
        layers: p.layers.clone(),
      })
    })
    .collect();
  let bucket = 2.0;
  let bw = ((outline.width() / bucket).ceil() as usize).max(1);
  let bh = ((outline.height() / bucket).ceil() as usize).max(1);
  let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); bw * bh];
  for (k, p) in pads.iter().enumerate() {
    let (bx, by) = (
      (((p.centre.x - x0) / bucket).floor().max(0.0) as usize).min(bw - 1),
      (((p.centre.y - y0) / bucket).floor().max(0.0) as usize).min(bh - 1),
    );
    if let Some(b) = buckets.get_mut(by * bw + bx) {
      b.push(k);
    }
  }
  let max_reach = pads.iter().map(|p| p.reach).fold(0.0, f64::max);
  let near = |p: Point, r: f64| -> Vec<usize> {
    let span = ((r + max_reach) / bucket).ceil() as i64 + 1;
    let (cx, cy) = (
      ((p.x - x0) / bucket).floor() as i64,
      ((p.y - y0) / bucket).floor() as i64,
    );
    let mut out = Vec::new();
    for by in (cy - span).max(0)..=(cy + span).min(bh as i64 - 1) {
      for bx in (cx - span).max(0)..=(cx + span).min(bw as i64 - 1) {
        if let Some(b) = buckets.get((by as usize) * bw + bx as usize) {
          out.extend(b.iter().copied());
        }
      }
    }
    out
  };
  let inside =
    |p: Point, m: f64| p.x >= x0 + m && p.x <= outline.max.x - m && p.y >= y0 + m && p.y <= outline.max.y - m;
  let keep = rules.edge() + rules.margin;
  let step = 0.05;
  let tiles = nx * ny;
  let mut cap_edge = vec![0u32; nl * tiles * 2];
  for ty in 0..ny {
    for tx in 0..nx {
      let t = ty * nx + tx;
      // Edge to the right: a vertical line at the tile's right side; edge down: a horizontal line.
      for dir in 0..2usize {
        if (dir == 0 && tx + 1 >= nx) || (dir == 1 && ty + 1 >= ny) {
          continue;
        }
        let (a, b) = if dir == 0 {
          let x = x0 + (tx + 1) as f64 * tile;
          (
            Point::new(x, y0 + ty as f64 * tile),
            Point::new(x, y0 + (ty + 1) as f64 * tile),
          )
        } else {
          let y = y0 + (ty + 1) as f64 * tile;
          (
            Point::new(x0 + tx as f64 * tile, y),
            Point::new(x0 + (tx + 1) as f64 * tile, y),
          )
        };
        let n = ((b - a).length() / step).floor() as usize + 1;
        let pts: Vec<Point> = (0..n)
          .map(|k| {
            let u = k as f64 * step / (b - a).length();
            Point::new(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u)
          })
          .collect();
        let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let cand = near(mid, tile);
        for (l, name) in layer_names.iter().enumerate() {
          let free: Vec<bool> = pts
            .iter()
            .map(|&p| {
              inside(p, rules.board_edge + rules.track / 2.0)
                && !cand.iter().any(|&k| {
                  pads
                    .get(k)
                    .is_some_and(|o| on_layer(&o.layers, name) && o.shape.distance(p) < keep)
                })
            })
            .collect();
          if let Some(c) = cap_edge.get_mut((l * tiles + t) * 2 + dir) {
            *c = tracks_in_runs(&free, step, rules.pitch() + rules.margin);
          }
        }
      }
    }
  }
  // Pad access: a net's own pad lets it out of its tile whatever its neighbours do, so the
  // edges around a tile that holds a netted pad carry at least one track on the pad's layers
  // (the tile model cannot see inside a tile, where the real access is).
  for f in &board.footprints {
    for p in &f.pads {
      if p.net.is_none_or(|n| n == 0) {
        continue;
      }
      let (tx, ty) = (
        (((p.at.x - x0) / tile).floor().max(0.0) as usize).min(nx - 1),
        (((p.at.y - y0) / tile).floor().max(0.0) as usize).min(ny - 1),
      );
      let t = ty * nx + tx;
      for (l, name) in layer_names.iter().enumerate() {
        if !on_layer(&p.layers, name) {
          continue;
        }
        let mut ids = vec![(l * tiles + t) * 2, (l * tiles + t) * 2 + 1];
        if tx > 0 {
          ids.push((l * tiles + t - 1) * 2);
        }
        if ty > 0 {
          ids.push((l * tiles + t - nx) * 2 + 1);
        }
        for e in ids {
          if let Some(c) = cap_edge.get_mut(e) {
            *c = (*c).max(1);
          }
        }
      }
    }
  }
  // Via capacity: legal sites on a 0.1 mm lattice, area over the hexagonal cell of one via.
  let need = via_dia / 2.0 + rules.clearance + rules.margin;
  let cell = 0.866 * via_spacing * via_spacing;
  let mut cap_via = vec![0u32; tiles];
  for ty in 0..ny {
    for tx in 0..nx {
      let (bx0, by0) = (x0 + tx as f64 * tile, y0 + ty as f64 * tile);
      let cand = near(Point::new(bx0 + tile / 2.0, by0 + tile / 2.0), tile);
      let n = (tile / 0.1).floor() as usize;
      let mut legal = 0usize;
      for iy in 0..n {
        for ix in 0..n {
          let p = Point::new(bx0 + (ix as f64 + 0.5) * 0.1, by0 + (iy as f64 + 0.5) * 0.1);
          if inside(p, rules.board_edge + via_dia / 2.0)
            && !cand
              .iter()
              .any(|&k| pads.get(k).is_some_and(|o| o.shape.distance(p) < need))
          {
            legal += 1;
          }
        }
      }
      if let Some(c) = cap_via.get_mut(ty * nx + tx) {
        *c = ((legal as f64 * 0.01) / cell).floor() as u32;
      }
    }
  }
  Graph {
    nx,
    ny,
    nl,
    cap_edge,
    cap_via,
    x0,
    y0,
    tile,
  }
}

/// A net to route on the graph: the (tile, layers) of each pad; plane nets only need vias.
struct NetDemand {
  net: u32,
  pads: Vec<(usize, Vec<usize>)>,
  plane: bool,
}

/// What one net uses: edge ids and via tiles.
#[derive(Default, Clone)]
struct Used {
  edges: Vec<usize>,
  vias: Vec<usize>,
}

/// Layer names for `layers` signal layers: the board's own signal layers first, then empty ones.
fn layer_list(signal_layers: &[String], layers: usize) -> Vec<String> {
  (0..layers)
    .map(|i| {
      signal_layers
        .get(i)
        .cloned()
        .unwrap_or_else(|| format!("Extra{}.Cu", i + 1 - signal_layers.len().min(i + 1)))
    })
    .collect()
}

/// Predict, for each layer count in `counts`, how much of the board's routing the tile model cannot place.
pub fn predict(
  board: &Board,
  rules: &CapacityRules,
  via: (f64, f64),
  signal_layers: &[String],
  plane_nets: &[u32],
  counts: &[usize],
  params: CongestionParams,
) -> Vec<Prediction> {
  let Some(outline) = board.outline else {
    return Vec::new();
  };
  let mut out = Vec::new();
  for &layers in counts {
    let names = layer_list(signal_layers, layers.max(1));
    let g = build_graph(board, rules, via, &names, outline, params.tile);
    // Nets: pad tiles and the layers each pad reaches.
    let mut by_net: HashMap<u32, Vec<(usize, Vec<usize>)>> = HashMap::new();
    for f in &board.footprints {
      for p in &f.pads {
        let Some(n) = p.net.filter(|&n| n != 0) else {
          continue;
        };
        // A through-hole pad on a plane net reaches the plane through its own hole: no via.
        if plane_nets.contains(&n) && p.layers.iter().any(|l| l == "*.Cu" || l == "F&B.Cu") {
          continue;
        }
        let (tx, ty) = (
          (((p.at.x - g.x0) / g.tile).floor().max(0.0) as usize).min(g.nx - 1),
          (((p.at.y - g.y0) / g.tile).floor().max(0.0) as usize).min(g.ny - 1),
        );
        let ls: Vec<usize> = names
          .iter()
          .enumerate()
          .filter(|(_, name)| on_layer(&p.layers, name))
          .map(|(l, _)| l)
          .collect();
        by_net.entry(n).or_default().push((ty * g.nx + tx, ls));
      }
    }
    let mut demands: Vec<NetDemand> = by_net
      .into_iter()
      .filter(|(n, pads)| pads.len() >= if plane_nets.contains(n) { 1 } else { 2 })
      .map(|(net, pads)| NetDemand {
        net,
        plane: plane_nets.contains(&net),
        pads,
      })
      .collect();
    demands.sort_by_key(|d| d.net);
    out.push(negotiate(&g, &demands, layers.max(1), params));
  }
  out
}

fn negotiate(g: &Graph, demands: &[NetDemand], layers: usize, params: CongestionParams) -> Prediction {
  let tiles = g.tiles();
  let mut use_edge = vec![0u32; g.cap_edge.len()];
  let mut use_via = vec![0u32; tiles];
  let mut hist_edge = vec![0.0f32; g.cap_edge.len()];
  let mut hist_via = vec![0.0f32; tiles];
  let mut used: Vec<Used> = vec![Used::default(); demands.len()];
  let mut pres = 0.5f32;
  let mut iterations = 0usize;
  let mut best_over = u32::MAX;
  let mut best_used: Vec<Used> = used.clone();
  for it in 0..params.iters.max(1) {
    iterations = it + 1;
    for (k, d) in demands.iter().enumerate() {
      if let Some(old) = used.get(k) {
        for &e in &old.edges {
          if let Some(u) = use_edge.get_mut(e) {
            *u = u.saturating_sub(1);
          }
        }
        for &v in &old.vias {
          if let Some(u) = use_via.get_mut(v) {
            *u = u.saturating_sub(1);
          }
        }
      }
      let new = route_net(
        g,
        d,
        &use_edge,
        &use_via,
        &hist_edge,
        &hist_via,
        pres,
        params.via_cost,
      );
      for &e in &new.edges {
        if let Some(u) = use_edge.get_mut(e) {
          *u += 1;
        }
      }
      for &v in &new.vias {
        if let Some(u) = use_via.get_mut(v) {
          *u += 1;
        }
      }
      if let Some(slot) = used.get_mut(k) {
        *slot = new;
      }
    }
    let over: u32 = use_edge
      .iter()
      .zip(&g.cap_edge)
      .map(|(&u, &c)| u.saturating_sub(c))
      .sum::<u32>()
      + use_via
        .iter()
        .zip(&g.cap_via)
        .map(|(&u, &c)| u.saturating_sub(c))
        .sum::<u32>();
    if over < best_over {
      best_over = over;
      best_used = used.clone();
    }
    if over == 0 {
      break;
    }
    for ((h, &u), &c) in hist_edge.iter_mut().zip(&use_edge).zip(&g.cap_edge) {
      if u > c {
        *h += 1.0;
      }
    }
    for ((h, &u), &c) in hist_via.iter_mut().zip(&use_via).zip(&g.cap_via) {
      if u > c {
        *h += 1.0;
      }
    }
    pres = (pres * 1.4).min(40.0);
  }
  // Report the best iteration.
  let mut use_edge = vec![0u32; g.cap_edge.len()];
  let mut use_via = vec![0u32; tiles];
  for u in &best_used {
    for &e in &u.edges {
      if let Some(x) = use_edge.get_mut(e) {
        *x += 1;
      }
    }
    for &v in &u.vias {
      if let Some(x) = use_via.get_mut(v) {
        *x += 1;
      }
    }
  }
  let mut p = Prediction {
    layers,
    per_layer_overflow: vec![0; layers],
    nets: demands.len(),
    iterations,
    tiles: (g.nx, g.ny),
    via_capacity: g.cap_via.iter().sum(),
    vias_used: use_via.iter().sum(),
    ..Prediction::default()
  };
  for (e, (&u, &c)) in use_edge.iter().zip(&g.cap_edge).enumerate() {
    if u > c {
      let (l, rest) = (e / 2 / tiles, e / 2 % tiles);
      p.overflow += u - c;
      p.overflow_edges += 1;
      if let Some(x) = p.per_layer_overflow.get_mut(l) {
        *x += u - c;
      }
      p.hotspots.push(Hotspot {
        at: g.centre(rest),
        layer: Some(l),
        what: if e % 2 == 0 {
          "edge to the right"
        } else {
          "edge below"
        },
        usage: u,
        capacity: c,
      });
    }
  }
  for (t, (&u, &c)) in use_via.iter().zip(&g.cap_via).enumerate() {
    if u > c {
      p.overflow += u - c;
      p.overflow_via_tiles += 1;
      p.hotspots.push(Hotspot {
        at: g.centre(t),
        layer: None,
        what: "vias",
        usage: u,
        capacity: c,
      });
    }
  }
  p.hotspots.sort_by(|a, b| {
    (b.usage - b.capacity)
      .cmp(&(a.usage - a.capacity))
      .then(a.at.x.total_cmp(&b.at.x))
      .then(a.at.y.total_cmp(&b.at.y))
  });
  p.hotspots.truncate(12);
  p
}

#[allow(clippy::too_many_arguments)]
fn route_net(
  g: &Graph,
  d: &NetDemand,
  use_edge: &[u32],
  use_via: &[u32],
  hist_edge: &[f32],
  hist_via: &[f32],
  pres: f32,
  via_cost: f32,
) -> Used {
  let mut used = Used::default();
  let tiles = g.tiles();
  if d.plane {
    // Each pad takes one via in the best tile among its own and its eight neighbours
    // (a tile full of pads may have no legal via site; the via goes next door).
    for (t, _) in &d.pads {
      let (tx, ty) = ((*t % g.nx) as i64, (*t / g.nx) as i64);
      let mut best: Option<(f32, usize)> = None;
      for dy in -1i64..=1 {
        for dx in -1i64..=1 {
          let (x, y) = (tx + dx, ty + dy);
          if x < 0 || y < 0 || x >= g.nx as i64 || y >= g.ny as i64 {
            continue;
          }
          let u = y as usize * g.nx + x as usize;
          let (used_n, cap) = (
            use_via.get(u).copied().unwrap_or(0) + used.vias.iter().filter(|&&v| v == u).count() as u32,
            g.cap_via.get(u).copied().unwrap_or(0),
          );
          let over = (used_n + 1).saturating_sub(cap) as f32;
          let cost = (dx.abs() + dy.abs()) as f32 + pres * over + hist_via.get(u).copied().unwrap_or(0.0);
          if best.is_none_or(|(c, _)| cost < c) {
            best = Some((cost, u));
          }
        }
      }
      used.vias.push(best.map_or(*t, |(_, u)| u));
    }
    return used;
  }
  let node = |l: usize, t: usize| l * tiles + t;
  let mut tree: HashSet<usize> = HashSet::new();
  let Some((t0, l0)) = d.pads.first() else {
    return used;
  };
  for &l in l0 {
    tree.insert(node(l, *t0));
  }
  let mut tree_tiles: Vec<usize> = vec![*t0];
  let mut remaining: Vec<usize> = (1..d.pads.len()).collect();
  let mut edge_set: HashSet<usize> = HashSet::new();
  let mut via_set: HashSet<usize> = HashSet::new();
  while !remaining.is_empty() {
    // Nearest remaining pad (tile distance) to the tree.
    let dist = |t: usize, u: usize| -> usize {
      let (a, b) = ((t % g.nx) as i64, (t / g.nx) as i64);
      let (c, e) = ((u % g.nx) as i64, (u / g.nx) as i64);
      ((a - c).abs() + (b - e).abs()) as usize
    };
    let pick = remaining
      .iter()
      .copied()
      .min_by_key(|&k| {
        d.pads.get(k).map_or(usize::MAX, |(t, _)| {
          tree_tiles
            .iter()
            .map(|&u| dist(*t, u))
            .min()
            .unwrap_or(usize::MAX)
        })
      })
      .unwrap_or(0);
    remaining.retain(|&k| k != pick);
    let Some((target_tile, target_layers)) = d.pads.get(pick) else {
      continue;
    };
    let targets: HashSet<usize> = target_layers.iter().map(|&l| node(l, *target_tile)).collect();
    // Dijkstra from the tree to any target node.
    let mut best: HashMap<usize, f32> = HashMap::new();
    let mut prev: HashMap<usize, usize> = HashMap::new();
    let mut heap: BinaryHeap<(Reverse<u32>, usize)> = BinaryHeap::new();
    for &s in &tree {
      best.insert(s, 0.0);
      heap.push((Reverse(0), s));
    }
    let mut end: Option<usize> = None;
    while let Some((Reverse(kd), n)) = heap.pop() {
      let dcur = best.get(&n).copied().unwrap_or(f32::INFINITY);
      if (kd as f32 / 64.0) > dcur + 0.02 {
        continue;
      }
      if targets.contains(&n) {
        end = Some(n);
        break;
      }
      let (l, t) = (n / tiles, n % tiles);
      let (tx, ty) = (t % g.nx, t / g.nx);
      let mut nbrs: Vec<(usize, f32, Option<usize>, Option<usize>)> = Vec::new(); // (node, cost, edge id, via tile)
      let edge_cost = |e: usize| -> f32 {
        let (u, c) = (
          use_edge.get(e).copied().unwrap_or(0),
          g.cap_edge.get(e).copied().unwrap_or(0),
        );
        let over = (u + 1).saturating_sub(c) as f32;
        1.0 + pres * over + hist_edge.get(e).copied().unwrap_or(0.0)
      };
      if tx + 1 < g.nx {
        let e = (l * tiles + t) * 2;
        nbrs.push((node(l, t + 1), edge_cost(e), Some(e), None));
      }
      if tx > 0 {
        let e = (l * tiles + t - 1) * 2;
        nbrs.push((node(l, t - 1), edge_cost(e), Some(e), None));
      }
      if ty + 1 < g.ny {
        let e = (l * tiles + t) * 2 + 1;
        nbrs.push((node(l, t + g.nx), edge_cost(e), Some(e), None));
      }
      if ty > 0 {
        let e = (l * tiles + t - g.nx) * 2 + 1;
        nbrs.push((node(l, t - g.nx), edge_cost(e), Some(e), None));
      }
      let (vu, vc) = (
        use_via.get(t).copied().unwrap_or(0),
        g.cap_via.get(t).copied().unwrap_or(0),
      );
      let vover = (vu + 1).saturating_sub(vc) as f32;
      let vcost = via_cost + pres * vover + hist_via.get(t).copied().unwrap_or(0.0);
      for l2 in 0..g.nl {
        if l2 != l {
          nbrs.push((node(l2, t), vcost, None, Some(t)));
        }
      }
      for (m, c, _, _) in nbrs {
        let nd = dcur + c;
        if nd < best.get(&m).copied().unwrap_or(f32::INFINITY) {
          best.insert(m, nd);
          prev.insert(m, n);
          heap.push((Reverse((nd * 64.0) as u32), m));
        }
      }
    }
    let Some(mut cur) = end else { continue };
    tree_tiles.push(*target_tile);
    tree.insert(cur);
    while let Some(&p) = prev.get(&cur) {
      let (l, t) = (cur / tiles, cur % tiles);
      let (pl, pt) = (p / tiles, p % tiles);
      if pl != l {
        via_set.insert(t);
      } else {
        // The edge between the two tiles.
        let e = if pt + 1 == t {
          (l * tiles + pt) * 2
        } else if t + 1 == pt {
          (l * tiles + t) * 2
        } else if pt + g.nx == t {
          (l * tiles + pt) * 2 + 1
        } else {
          (l * tiles + t) * 2 + 1
        };
        edge_set.insert(e);
      }
      tree.insert(p);
      tree_tiles.push(pt);
      cur = p;
      if best.get(&p).copied().unwrap_or(1.0) == 0.0 {
        break;
      }
    }
  }
  used.edges = edge_set.into_iter().collect();
  used.vias = via_set.into_iter().collect();
  used.edges.sort_unstable();
  used.vias.sort_unstable();
  used
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rules() -> CapacityRules {
    CapacityRules {
      track: 0.1524,
      clearance: 0.1524,
      margin: 0.005,
      board_edge: 0.3,
    }
  }

  #[test]
  fn open_runs_count_tracks() {
    // 20 samples 0.05 apart = 0.95 mm free: floor(0.95 / 0.3048) + 1 = 4 tracks.
    assert_eq!(tracks_in_runs(&[true; 20], 0.05, 0.3048), 4);
    // Two stretches of 3 samples (0.1 mm) each: one track each.
    let mut f = vec![false; 12];
    for k in [1usize, 2, 3, 7, 8, 9] {
      f[k] = true;
    }
    assert_eq!(tracks_in_runs(&f, 0.05, 0.3048), 2);
    assert_eq!(tracks_in_runs(&[], 0.05, 0.3048), 0);
    assert_eq!(tracks_in_runs(&[false; 5], 0.05, 0.3048), 0);
  }

  #[test]
  fn layer_names_extend_with_empty_layers() {
    let sig = vec!["F.Cu".to_string(), "B.Cu".to_string()];
    assert_eq!(layer_list(&sig, 1), vec!["F.Cu"]);
    assert_eq!(layer_list(&sig, 2), vec!["F.Cu", "B.Cu"]);
    let l4 = layer_list(&sig, 4);
    assert_eq!(l4.len(), 4);
    assert_eq!(&l4[..2], &sig[..]);
    assert!(l4[2].starts_with("Extra") && l4[3].starts_with("Extra") && l4[2] != l4[3]);
  }

  /// Six nets each cross a wall of pads that leaves a gap holding only two
  /// tracks per layer: one layer cannot carry them, three layers can.
  #[test]
  fn a_wall_with_a_narrow_gap_needs_more_layers() {
    // A wall at x = 15 of two big pads on every layer leaving a 1.0 mm gap (y from 9.5 to 10.5):
    // floor((1.0 - 0.4572) / 0.3048) + 1 = 2 tracks per layer through it.
    let pads = r#"(pad "w1" smd rect (at 15 4.75) (size 1.0 9.5) (layers "*.Cu")) (pad "w2" smd rect (at 15 15.25) (size 1.0 9.5) (layers "*.Cu"))"#;
    let mut left = String::new();
    let mut right = String::new();
    for k in 0..6 {
      let y = 7.5 + k as f64 * 1.0;
      left += &format!(
        r#"(pad "{k}" smd rect (at 5 {y}) (size 0.5 0.5) (layers "F.Cu") (net {n} "N{n}"))"#,
        n = k + 1
      );
      right += &format!(
        r#"(pad "{k}" smd rect (at 25 {y}) (size 0.5 0.5) (layers "F.Cu") (net {n} "N{n}"))"#,
        n = k + 1
      );
    }
    let src = format!(
      r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "N1") (net 2 "N2") (net 3 "N3") (net 4 "N4") (net 5 "N5") (net 6 "N6")
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts"))
  (gr_line (start 30 0) (end 30 20) (layer "Edge.Cuts"))
  (gr_line (start 30 20) (end 0 20) (layer "Edge.Cuts"))
  (gr_line (start 0 20) (end 0 0) (layer "Edge.Cuts"))
  (footprint "L" (layer "F.Cu") (at 0 0) (property "Reference" "L1") {left})
  (footprint "R" (layer "F.Cu") (at 0 0) (property "Reference" "R1") {right})
  (footprint "W" (layer "F.Cu") (at 0 0) (property "Reference" "W1") {pads})
)"#
    );
    let doc = autolayout_sexpr::parse(&src).unwrap();
    let board = Board::read(&doc).unwrap();
    let sig = vec!["F.Cu".to_string(), "B.Cu".to_string()];
    let p = predict(
      &board,
      &rules(),
      (0.6096, 0.77),
      &sig,
      &[],
      &[1, 2, 3, 4],
      CongestionParams {
        tile: 1.0,
        iters: 40,
        via_cost: 2.0,
      },
    );
    assert_eq!(p.len(), 4);
    // The wall pads are on every layer, so the gap holds 2 tracks per layer, whatever the layer count.
    assert!(p[0].overflow > 0, "1 layer: {:?}", p[0]);
    // More layers carry more tracks through the same gap: overflow never grows with layers
    // and reaches zero by 4 layers (4 x 2 = 8 >= 6 tracks).
    for w in p.windows(2) {
      assert!(w[1].overflow <= w[0].overflow, "{:?} then {:?}", w[0], w[1]);
    }
    assert_eq!(p[3].overflow, 0, "4 layers: {:?}", p[3]);
  }
}
