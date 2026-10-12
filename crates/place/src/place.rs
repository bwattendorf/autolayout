//! M4: a legal placer. Deterministic: same input, same output, no seed.
//!
//! **Global placement.** Every movable part is pulled toward the centroid of
//! each net it is on (the "star" model), using its real pad offsets, by
//! damped Gauss-Seidel iteration. Fixed (locked) parts are anchors. Plane
//! nets (more than [`PLANE_FANOUT`] pads) are left out: a pour serves them,
//! and their centroid is the middle of the board, which would drag every
//! part there. Group bonds add pull: a bypass cap toward its one IC, a
//! converter's parts toward its regulator, a crystal's parts toward its host.
//!
//! **Legalisation.** Parts are placed one at a time — MCU first, then by pad
//! count, then by courtyard area — each at the free position nearest its
//! global target, searched in rings outward. "Free" is the same exact
//! courtyard-outline test the grader uses (concave outlines kept), against
//! every already-placed part on the same side, and the courtyard must stay
//! inside the outline box by [`EDGE_MARGIN_MM`]. Sides and rotations are
//! kept as they are; fixed parts never move.
//!
//! What it does not do yet (docs/gaps.md): rotate, flip sides, look at
//! routing congestion, or apply circuit rules (switch-loop size, crystal
//! trace length) beyond the group bonds.

use std::collections::HashMap;

use autolayout_board::{BBox, Board, Point, Seg, outlines_overlap};

use crate::group::{Grouping, Role};

/// Nets with more pads are planes and get no spring (see module notes).
pub const PLANE_FANOUT: usize = 16;
/// Global-placement iterations and damping.
const ITERATIONS: usize = 300;
const DAMPING: f64 = 0.5;
/// Extra weight of a group bond relative to one net spring.
const BOND: f64 = 4.0;
/// Legalisation search: ring spacing and how far out to look.
const STEP_MM: f64 = 0.25;
const MAX_RADIUS_MM: f64 = 150.0;
/// Courtyards stay this far inside the outline box.
pub const EDGE_MARGIN_MM: f64 = 0.5;
/// Edge contact closer than this counts as touching, not overlapping.
const TOUCH_MM: f64 = 1e-3;

#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
  /// (footprint index, new position). Rotation is unchanged.
  pub moves: Vec<(usize, Point)>,
  /// Movable parts no legal spot was found for within the search radius.
  pub unplaced: Vec<usize>,
}

pub fn place(board: &Board, g: &Grouping) -> Placement {
  let n = board.footprints.len();
  let Some(outline) = board.outline else {
    return Placement {
      moves: Vec::new(),
      unplaced: (0..n).collect(),
    };
  };
  let movable: Vec<bool> = (0..n).map(|f| g.role.get(f) != Some(&Role::Fixed)).collect();

  // Pad offsets from each part's origin, in board orientation (rotation is
  // kept, so these are constant while the part moves).
  let offsets: Vec<Vec<(Point, Option<u32>)>> = board
    .footprints
    .iter()
    .map(|fp| fp.pads.iter().map(|p| (p.at - fp.at, p.net)).collect())
    .collect();

  // Nets that get a spring: 2..=PLANE_FANOUT pads.
  let mut net_size: HashMap<u32, usize> = HashMap::new();
  for pads in &offsets {
    for (_, net) in pads {
      if let Some(n) = net {
        *net_size.entry(*n).or_default() += 1;
      }
    }
  }
  let sprung = |net: &u32| {
    net_size
      .get(net)
      .is_some_and(|&k| (2..=PLANE_FANOUT).contains(&k))
  };

  let bonds = group_bonds(g);
  let centre = Point::new(
    (outline.min.x + outline.max.x) / 2.0,
    (outline.min.y + outline.max.y) / 2.0,
  );

  // Start every movable part at the board centre: staging-grid positions sit
  // off the board and would bias the first iterations outward.
  let mut pos: Vec<Point> = board
    .footprints
    .iter()
    .enumerate()
    .map(|(f, fp)| if movable[f] { centre } else { fp.at })
    .collect();

  for _ in 0..ITERATIONS {
    // Net centroids from current positions.
    let mut sum: HashMap<u32, (Point, f64)> = HashMap::new();
    for (f, pads) in offsets.iter().enumerate() {
      for &(off, net) in pads {
        if let Some(net) = net.filter(|n| sprung(n)) {
          let e = sum.entry(net).or_insert((Point::default(), 0.0));
          e.0 = e.0 + pos[f] + off;
          e.1 += 1.0;
        }
      }
    }
    for f in (0..n).filter(|&f| movable[f]) {
      let mut acc = Point::default();
      let mut w = 0.0;
      for &(off, net) in &offsets[f] {
        if let Some((s, k)) = net.filter(|n| sprung(n)).and_then(|n| sum.get(&n)) {
          let c = Point::new(s.x / k, s.y / k);
          acc = acc + (c - off);
          w += 1.0;
        }
      }
      for &(other, weight) in bonds.get(&f).map_or(&[][..], Vec::as_slice) {
        acc = acc + Point::new(pos[other].x * weight, pos[other].y * weight);
        w += weight;
      }
      if w > 0.0 {
        let target = Point::new(acc.x / w, acc.y / w);
        pos[f] = Point::new(
          pos[f].x + DAMPING * (target.x - pos[f].x),
          pos[f].y + DAMPING * (target.y - pos[f].y),
        );
      }
    }
  }

  // Legalise.
  let edges: Vec<Vec<Seg>> = board
    .footprints
    .iter()
    .map(|fp| {
      fp.courtyard_edges_local
        .iter()
        .map(|&(a, b)| (fp.to_board(a) - fp.at, fp.to_board(b) - fp.at))
        .collect()
    })
    .collect();
  let mut placed: Vec<(usize, Point)> = (0..n).filter(|&f| !movable[f]).map(|f| (f, pos[f])).collect();
  let mut order: Vec<usize> = (0..n).filter(|&f| movable[f]).collect();
  let area = |f: usize| {
    BBox::from_points(edges[f].iter().flat_map(|&(a, b)| [a, b])).map_or(0.0, |b| b.width() * b.height())
  };
  order.sort_by(|&a, &b| {
    let key = |f: usize| {
      (
        g.role.get(f) != Some(&Role::Mcu),
        std::cmp::Reverse(board.footprints[f].pads.len()),
      )
    };
    key(a)
      .cmp(&key(b))
      .then(area(b).total_cmp(&area(a)))
      .then(a.cmp(&b))
  });

  let inner = BBox {
    min: Point::new(outline.min.x + EDGE_MARGIN_MM, outline.min.y + EDGE_MARGIN_MM),
    max: Point::new(outline.max.x - EDGE_MARGIN_MM, outline.max.y - EDGE_MARGIN_MM),
  };
  // A through-hole part occupies BOTH sides: its pins come out of the other
  // face. Treating it as one-sided let the first version drop the SWD header
  // (J11) through a back-side bypass cap (C14) on m1 — courtyards on
  // different layers never meet, and KiCad's DRC reported the pad clearance.
  let tht: Vec<bool> = board
    .footprints
    .iter()
    .map(|fp| fp.attrs.iter().any(|a| a == "through_hole") || fp.pads.iter().any(|p| p.kind == "thru_hole"))
    .collect();
  let mut moves = Vec::new();
  let mut unplaced = Vec::new();
  for f in order {
    let side = board.footprints[f].side;
    let fits = |at: Point| {
      let mine: Vec<Seg> = edges[f].iter().map(|&(a, b)| (a + at, b + at)).collect();
      let Some(bb) =
        BBox::from_points(mine.iter().flat_map(|&(a, b)| [a, b])).or(Some(BBox { min: at, max: at }))
      else {
        return false;
      };
      if bb.min.x < inner.min.x || bb.min.y < inner.min.y || bb.max.x > inner.max.x || bb.max.y > inner.max.y
      {
        return false;
      }
      placed.iter().all(|&(o, opos)| {
        let shares_a_side = board.footprints[o].side == side || tht[f] || tht[o];
        if !shares_a_side || edges[o].is_empty() || mine.is_empty() {
          return true;
        }
        let theirs: Vec<Seg> = edges[o].iter().map(|&(a, b)| (a + opos, b + opos)).collect();
        let ob = BBox::from_points(theirs.iter().flat_map(|&(a, b)| [a, b]));
        let apart = ob.is_none_or(|ob| {
          ob.max.x <= bb.min.x || ob.min.x >= bb.max.x || ob.max.y <= bb.min.y || ob.min.y >= bb.max.y
        });
        apart || !outlines_overlap(&mine, &theirs, TOUCH_MM)
      })
    };
    match nearest_free(pos[f], &fits) {
      Some(at) => {
        placed.push((f, at));
        moves.push((f, at));
      }
      None => unplaced.push(f),
    }
  }
  moves.sort_by_key(|&(f, _)| f);
  Placement { moves, unplaced }
}

/// Group bonds as (other part, weight) per part, both directions.
fn group_bonds(g: &Grouping) -> HashMap<usize, Vec<(usize, f64)>> {
  let mut bonds: HashMap<usize, Vec<(usize, f64)>> = HashMap::new();
  let mut bond = |a: usize, b: usize| {
    if a != b {
      bonds.entry(a).or_default().push((b, BOND));
      bonds.entry(b).or_default().push((a, BOND));
    }
  };
  for b in &g.bypass {
    if let [ic] = b.ics.as_slice() {
      bond(b.cap, *ic);
    }
  }
  for c in &g.converters {
    let members = std::iter::once(c.inductor)
      .chain(c.boot.iter().copied())
      .chain(c.feedback.iter().copied())
      .chain(c.output_caps.iter().copied())
      .chain(c.input_caps.iter().flatten().copied());
    for m in members {
      bond(m, c.regulator);
    }
  }
  for x in &g.crystals {
    if let Some(h) = x.host {
      bond(x.part, h);
      for &c in &x.load_caps {
        bond(c, x.part);
      }
    }
  }
  bonds
}

/// A free position near `target`: rings of radius STEP, 2·STEP, … each
/// sampled every STEP along its circumference; the first fit found wins
/// (innermost ring first, then angle order from +x), so the result is within
/// one STEP of the nearest free spot and fully deterministic.
fn nearest_free(target: Point, fits: &dyn Fn(Point) -> bool) -> Option<Point> {
  if fits(target) {
    return Some(target);
  }
  let rings = (MAX_RADIUS_MM / STEP_MM) as usize;
  for k in 1..=rings {
    let r = k as f64 * STEP_MM;
    let samples = ((std::f64::consts::TAU * r / STEP_MM).ceil() as usize).max(8);
    for i in 0..samples {
      let a = std::f64::consts::TAU * i as f64 / samples as f64;
      let p = Point::new(target.x + r * a.cos(), target.y + r * a.sin());
      // Snap to a 0.05 mm grid so output coordinates are tidy and stable.
      let p = Point::new((p.x / 0.05).round() * 0.05, (p.y / 0.05).round() * 0.05);
      if fits(p) {
        return Some(p);
      }
    }
  }
  None
}
