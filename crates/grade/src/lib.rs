//! Placement metrics, computed from a board as read back off disk.
//!
//! The grader never sees a placer's in-memory state: it takes a [`Board`]
//! read from a written file, so it grades what KiCad will see, not what the
//! placer intended (CLAUDE.md rule 3). Each metric that KiCad can also report
//! is cross-checked against `kicad-cli pcb drc` in `tests/`.
//!
//! Approximations, stated rather than hidden:
//! - Courtyards are tested as their real outlines, edge against edge, concave
//!   shapes included (arcs and circles as 32-chord polylines). Not boxes and
//!   not convex hulls: an electrolytic can's courtyard is a cross, any convex
//!   stand-in fills its notches, and a part legally sitting in one reads as a
//!   collision. That exact false positive (m1 R6/C22) has now been made three
//!   times — Portcullis's `placement_review` with bounding boxes (retracted
//!   2026-09-01), and this grader twice on 2026-10-12, first with boxes, then
//!   with convex hulls — so the real C22 outline is a unit test in
//!   `autolayout-board`'s geometry.
//! - "On board" tests against the bounding box of Edge.Cuts, which is exact
//!   for rectangular outlines only.
//! - Wirelength is measured pad to pad in a straight line; no routing.

pub mod escape;

use std::collections::HashMap;

use autolayout_board::{BBox, Board, Footprint, Point, Seg, outlines_overlap};

/// Nets with more pads than this are treated as planes (supply and ground
/// pour) and reported separately: their wirelength is dominated by the pour,
/// not by placement. A structural threshold, not a name match (rule 2).
pub const PLANE_FANOUT: usize = 16;

/// Distance within which courtyard edges count as touching, not overlapping.
const TOUCH_MM: f64 = 1e-3;

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
  pub footprints: usize,
  pub without_courtyard: usize,
  /// Pairs of references whose courtyards overlap on the same side.
  pub overlaps: Vec<(String, String)>,
  /// References whose courtyard leaves the outline box (or the whole part
  /// when it has no courtyard).
  pub off_board: Vec<String>,
  /// Half-perimeter wirelength of nets with 2..=PLANE_FANOUT pads, mm.
  pub hpwl_signal: f64,
  /// Minimum-spanning-tree ratsnest length of the same nets, mm.
  pub ratsnest_signal: f64,
  /// Same two measures over the plane nets (> PLANE_FANOUT pads).
  pub hpwl_plane: f64,
  pub ratsnest_plane: f64,
  pub signal_nets: usize,
  pub plane_nets: usize,
}

/// Courtyard outline on the board: the local edges, transformed. Empty if the
/// footprint has no courtyard.
pub fn courtyard_edges(fp: &Footprint) -> Vec<Seg> {
  fp.courtyard_edges_local
    .iter()
    .map(|&(a, b)| (fp.to_board(a), fp.to_board(b)))
    .collect()
}

/// Courtyard box on the board (around the transformed outline).
pub fn courtyard(fp: &Footprint) -> Option<BBox> {
  BBox::from_points(courtyard_edges(fp).into_iter().flat_map(|(a, b)| [a, b]))
}

pub fn grade(board: &Board) -> Report {
  let polys: Vec<Vec<Seg>> = board.footprints.iter().map(courtyard_edges).collect();
  let boxes: Vec<Option<BBox>> = polys
    .iter()
    .map(|p| BBox::from_points(p.iter().flat_map(|&(a, b)| [a, b])))
    .collect();

  let mut overlaps = Vec::new();
  for (i, a) in board.footprints.iter().enumerate() {
    for (j, b) in board.footprints.iter().enumerate().skip(i + 1) {
      if a.side != b.side {
        continue;
      }
      // Cheap box rejection first; the outline test decides.
      let boxes_meet = matches!((boxes.get(i), boxes.get(j)), (Some(Some(ba)), Some(Some(bb))) if overlap_area(ba, bb) > 0.0);
      if boxes_meet
        && let (Some(pa), Some(pb)) = (polys.get(i), polys.get(j))
        && outlines_overlap(pa, pb, TOUCH_MM)
      {
        overlaps.push(ordered(&a.reference, &b.reference));
      }
    }
  }
  overlaps.sort();

  let off_board = match board.outline {
    None => Vec::new(),
    Some(outline) => board
      .footprints
      .iter()
      .zip(&boxes)
      .filter(|(fp, b)| {
        let extent = b.or_else(|| {
          BBox::from_points(fp.pads.iter().map(|p| p.at)).or(Some(BBox {
            min: fp.at,
            max: fp.at,
          }))
        });
        extent.is_some_and(|e| !inside(&e, &outline))
      })
      .map(|(fp, _)| fp.reference.clone())
      .collect(),
  };

  let mut by_net: HashMap<u32, Vec<Point>> = HashMap::new();
  for fp in &board.footprints {
    for pad in &fp.pads {
      if let Some(net) = pad.net {
        by_net.entry(net).or_default().push(pad.at);
      }
    }
  }
  let mut r = Report {
    footprints: board.footprints.len(),
    without_courtyard: boxes.iter().filter(|b| b.is_none()).count(),
    overlaps,
    off_board,
    hpwl_signal: 0.0,
    ratsnest_signal: 0.0,
    hpwl_plane: 0.0,
    ratsnest_plane: 0.0,
    signal_nets: 0,
    plane_nets: 0,
  };
  for pts in by_net.values().filter(|p| p.len() >= 2) {
    let (h, m) = (hpwl(pts), mst_length(pts));
    if pts.len() > PLANE_FANOUT {
      r.hpwl_plane += h;
      r.ratsnest_plane += m;
      r.plane_nets += 1;
    } else {
      r.hpwl_signal += h;
      r.ratsnest_signal += m;
      r.signal_nets += 1;
    }
  }
  r
}

fn ordered(a: &str, b: &str) -> (String, String) {
  if a <= b {
    (a.to_owned(), b.to_owned())
  } else {
    (b.to_owned(), a.to_owned())
  }
}

fn overlap_area(a: &BBox, b: &BBox) -> f64 {
  let w = a.max.x.min(b.max.x) - a.min.x.max(b.min.x);
  let h = a.max.y.min(b.max.y) - a.min.y.max(b.min.y);
  if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

fn inside(inner: &BBox, outer: &BBox) -> bool {
  inner.min.x >= outer.min.x
    && inner.min.y >= outer.min.y
    && inner.max.x <= outer.max.x
    && inner.max.y <= outer.max.y
}

fn hpwl(pts: &[Point]) -> f64 {
  BBox::from_points(pts.iter().copied()).map_or(0.0, |b| b.width() + b.height())
}

/// Prim's algorithm on the complete Euclidean graph, O(n²): fine for the pad
/// counts of a single net.
fn mst_length(pts: &[Point]) -> f64 {
  let n = pts.len();
  if n < 2 {
    return 0.0;
  }
  let mut in_tree = vec![false; n];
  let mut best = vec![f64::INFINITY; n];
  let mut total = 0.0;
  if let Some(b) = best.first_mut() {
    *b = 0.0;
  }
  for _ in 0..n {
    let mut pick = None;
    for i in 0..n {
      if in_tree.get(i) == Some(&false) && pick.is_none_or(|p: usize| best.get(i) < best.get(p)) {
        pick = Some(i);
      }
    }
    let Some(u) = pick else { break };
    if let Some(t) = in_tree.get_mut(u) {
      *t = true;
    }
    total += best.get(u).copied().unwrap_or(0.0);
    let Some(&pu) = pts.get(u) else { break };
    for (v, &pv) in pts.iter().enumerate() {
      if in_tree.get(v) == Some(&false) {
        let d = (pv - pu).length();
        if let Some(bv) = best.get_mut(v)
          && d < *bv
        {
          *bv = d;
        }
      }
    }
  }
  total
}

impl std::fmt::Display for Report {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    writeln!(
      f,
      "  footprints            {} ({} without courtyard)",
      self.footprints, self.without_courtyard
    )?;
    writeln!(f, "  courtyard overlaps    {}", self.overlaps.len())?;
    for (a, b) in &self.overlaps {
      writeln!(f, "      {a} / {b}")?;
    }
    writeln!(f, "  off board             {}", self.off_board.len())?;
    writeln!(
      f,
      "  signal nets ({:>3})     HPWL {:>9.1} mm   ratsnest {:>9.1} mm",
      self.signal_nets, self.hpwl_signal, self.ratsnest_signal
    )?;
    write!(
      f,
      "  plane nets  ({:>3})     HPWL {:>9.1} mm   ratsnest {:>9.1} mm  (> {PLANE_FANOUT} pads)",
      self.plane_nets, self.hpwl_plane, self.ratsnest_plane
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn mst_of_unit_square_is_three() {
    let pts = [
      Point::new(0.0, 0.0),
      Point::new(1.0, 0.0),
      Point::new(1.0, 1.0),
      Point::new(0.0, 1.0),
    ];
    assert!((mst_length(&pts) - 3.0).abs() < 1e-12);
    assert!((hpwl(&pts) - 2.0).abs() < 1e-12);
  }

  #[test]
  fn touching_boxes_do_not_overlap() {
    let a = BBox {
      min: Point::new(0.0, 0.0),
      max: Point::new(1.0, 1.0),
    };
    let b = BBox {
      min: Point::new(1.0, 0.0),
      max: Point::new(2.0, 1.0),
    };
    assert_eq!(overlap_area(&a, &b), 0.0);
  }
}
