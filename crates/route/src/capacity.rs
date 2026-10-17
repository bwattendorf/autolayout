//! Routing capacity of the space around a part (M7.5, 2026-10-16).
//!
//! A router cannot finish a board whose nets need more room than the copper
//! leaves. Two pieces of arithmetic say how much room there is.
//!
//! **A gap.** A track's centre-line must keep `track / 2 + clearance` from any
//! copper edge, and two centre-lines must be `track + clearance` apart. So a
//! gap of `g` mm between two copper edges holds
//! `floor((g - 2 * (track / 2 + clearance)) / (track + clearance)) + 1` tracks
//! on one layer (none when `g` is smaller than one track plus both clearances).
//! [`tracks_in_gap`] and [`gap_for_tracks`] are that formula and its inverse;
//! [`gap_for_nets`] is the gap two facing parts need so that `n` nets can pass
//! between them on `layers` signal layers.
//!
//! **A ring.** Every net with a pad inside a closed loop and a pad outside it
//! must cross the loop. A track crosses the loop at a point where its
//! centre-line clears every foreign pad on that layer; a free stretch of length
//! `L` along the loop holds at most `floor(L / (track + clearance)) + 1` tracks
//! on a layer (tracks crossing at right angles, the densest way). So the
//! number of nets that must cross is *demand*, the sum over layers and free
//! stretches is *capacity*, and **demand above capacity means no router can
//! finish the board**, whatever its quality. [`ring_reports`] evaluates this
//! for rings at several distances around one part, in two flavours:
//! - `capacity_strict`: every foreign pad blocks (what a router sees);
//! - `capacity_loose`: pads of the nets that must cross do not block, so a
//!   net may cross right at its own pad. This can only overstate the room, so
//!   demand above it is a proof, and demand above `strict` a strong warning.
//!
//! What this does not count: vias (a layer change takes room), plane nets
//! (they need a via inside the ring, not a crossing; they are left out of the
//! demand), tracks that cross obliquely, and nets that could be routed by going
//! round a different loop. It is an upper bound on capacity, not a router.

use std::collections::{BTreeMap, BTreeSet};

use autolayout_board::{BBox, Board, Point};

use crate::shape::{Shape, pad_copper};

/// The rule numbers the arithmetic uses (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapacityRules {
  pub track: f64,
  pub clearance: f64,
  /// Extra room kept on top of the rules by the router (`max_error`); only the
  /// strict capacity uses it.
  pub margin: f64,
  /// Copper edge to board edge (`min_copper_edge_clearance`).
  pub board_edge: f64,
}

impl CapacityRules {
  /// Least centre-line to centre-line distance of two tracks.
  pub fn pitch(&self) -> f64 {
    self.track + self.clearance
  }

  /// Least centre-line to copper-edge distance.
  pub fn edge(&self) -> f64 {
    self.track / 2.0 + self.clearance
  }
}

/// Tracks that fit side by side, on one layer, in a gap of `gap` mm between two
/// copper edges.
pub fn tracks_in_gap(gap: f64, r: &CapacityRules) -> usize {
  let room = gap - 2.0 * r.edge();
  if !room.is_finite() || room < -1e-9 {
    return 0;
  }
  (room.max(0.0) / r.pitch() + 1e-9).floor() as usize + 1
}

/// Least gap between two copper edges that holds `n` tracks side by side on one layer.
pub fn gap_for_tracks(n: usize, r: &CapacityRules) -> f64 {
  if n == 0 {
    return 0.0;
  }
  2.0 * r.edge() + (n - 1) as f64 * r.pitch()
}

/// Least gap two facing parts need so that `nets` nets can pass between them on
/// `layers` signal layers, the nets spread evenly over the layers.
pub fn gap_for_nets(nets: usize, layers: usize, r: &CapacityRules) -> f64 {
  gap_for_tracks(nets.div_ceil(layers.max(1)), r)
}

/// Demand and capacity of one ring around a part.
#[derive(Debug, Clone, PartialEq)]
pub struct RingReport {
  /// Distance from the part's pad box to the ring (mm).
  pub offset: f64,
  pub perimeter: f64,
  /// Net codes that must cross the ring (a pad inside and a pad outside), plane nets left out.
  pub crossing: Vec<u32>,
  /// Tracks the ring can pass when every foreign pad blocks, per signal layer.
  pub strict: Vec<usize>,
  /// As `strict`, but a net's own pads do not block it.
  pub loose: Vec<usize>,
}

impl RingReport {
  pub fn demand(&self) -> usize {
    self.crossing.len()
  }

  pub fn capacity_strict(&self) -> usize {
    self.strict.iter().sum()
  }

  pub fn capacity_loose(&self) -> usize {
    self.loose.iter().sum()
  }
}

fn on_layer(pad_layers: &[String], layer: &str) -> bool {
  pad_layers
    .iter()
    .any(|l| l == layer || l == "*.Cu" || (l == "F&B.Cu" && (layer == "F.Cu" || layer == "B.Cu")))
}

/// Rings around footprint `fi` at each of `offsets`. `signal_layers` are the
/// copper layers a signal may use (plane layers left out); `plane_nets` are the
/// net codes poured as planes; `outline` is the board's bounding box.
pub fn ring_reports(
  board: &Board,
  fi: usize,
  offsets: &[f64],
  rules: &CapacityRules,
  signal_layers: &[String],
  plane_nets: &[u32],
  outline: Option<BBox>,
) -> Vec<RingReport> {
  let Some(fp) = board.footprints.get(fi) else {
    return Vec::new();
  };
  let part_box = BBox::from_points(
    fp.pads
      .iter()
      .filter_map(|p| pad_copper(p).bbox())
      .flat_map(|b| [b.min, b.max]),
  );
  let Some(part_box) = part_box else {
    return Vec::new();
  };
  // Every pad of every other footprint: its copper, layers and net.
  struct Foreign {
    shape: Shape,
    layers: Vec<String>,
    net: Option<u32>,
    centre: Point,
    reach: f64,
  }
  let foreign: Vec<Foreign> = board
    .footprints
    .iter()
    .enumerate()
    .filter(|&(i, _)| i != fi)
    .flat_map(|(_, f)| f.pads.iter())
    .filter(|p| p.kind != "np_thru_hole" || p.drill.is_some())
    .filter_map(|p| {
      let shape = pad_copper(p);
      let bb = shape.bbox()?;
      Some(Foreign {
        reach: bb.width().hypot(bb.height()) / 2.0,
        centre: Point::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0),
        shape,
        layers: p.layers.clone(),
        net: p.net,
      })
    })
    .collect();
  // Net -> pad centres, to find the nets that cross a ring.
  let mut net_pads: BTreeMap<u32, Vec<Point>> = BTreeMap::new();
  for f in &board.footprints {
    for p in &f.pads {
      if let Some(n) = p.net.filter(|&n| n != 0 && !plane_nets.contains(&n)) {
        net_pads.entry(n).or_default().push(p.at);
      }
    }
  }
  let step = 0.05;
  let mut out = Vec::new();
  for &d in offsets {
    let (x0, y0, x1, y1) = (
      part_box.min.x - d,
      part_box.min.y - d,
      part_box.max.x + d,
      part_box.max.y + d,
    );
    let inside = |p: Point| p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1;
    let crossing: Vec<u32> = net_pads
      .iter()
      .filter(|(_, pts)| pts.iter().any(|&p| inside(p)) && pts.iter().any(|&p| !inside(p)))
      .map(|(&n, _)| n)
      .collect();
    let crossing_set: BTreeSet<u32> = crossing.iter().copied().collect();
    // Points along the ring, one every `step`, clockwise from the top-left corner.
    let mut pts: Vec<Point> = Vec::new();
    let sides = [
      (Point::new(x0, y0), Point::new(x1, y0)),
      (Point::new(x1, y0), Point::new(x1, y1)),
      (Point::new(x1, y1), Point::new(x0, y1)),
      (Point::new(x0, y1), Point::new(x0, y0)),
    ];
    let mut perimeter = 0.0;
    for (a, b) in sides {
      let len = (b - a).length();
      perimeter += len;
      let n = (len / step).floor() as usize;
      for k in 0..n {
        let t = k as f64 * step / len;
        pts.push(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t));
      }
    }
    // Pads near the ring only.
    let band = rules.edge() + rules.margin + 0.1;
    let near: Vec<&Foreign> = foreign
      .iter()
      .filter(|f| {
        let dx = (f.centre.x - x0).abs().min((f.centre.x - x1).abs());
        let dy = (f.centre.y - y0).abs().min((f.centre.y - y1).abs());
        let in_x = f.centre.x >= x0 - f.reach - band && f.centre.x <= x1 + f.reach + band;
        let in_y = f.centre.y >= y0 - f.reach - band && f.centre.y <= y1 + f.reach + band;
        in_x && in_y && (dx <= f.reach + band || dy <= f.reach + band)
      })
      .collect();
    let board_ok = |p: Point| {
      outline.is_none_or(|o| {
        let m = rules.board_edge + rules.track / 2.0;
        p.x >= o.min.x + m && p.x <= o.max.x - m && p.y >= o.min.y + m && p.y <= o.max.y - m
      })
    };
    let capacity = |strict: bool, layer: &str| -> usize {
      let keep = rules.edge() + if strict { rules.margin } else { 0.0 };
      let free: Vec<bool> = pts
        .iter()
        .map(|&p| {
          board_ok(p)
            && !near.iter().any(|f| {
              on_layer(&f.layers, layer)
                && (strict || !f.net.is_some_and(|n| crossing_set.contains(&n)))
                && f.shape.distance(p) < keep
            })
        })
        .collect();
      runs(
        &free,
        step,
        if strict {
          rules.pitch() + rules.margin
        } else {
          rules.pitch()
        },
      )
    };
    let strict: Vec<usize> = signal_layers.iter().map(|l| capacity(true, l)).collect();
    let loose: Vec<usize> = signal_layers.iter().map(|l| capacity(false, l)).collect();
    out.push(RingReport {
      offset: d,
      perimeter,
      crossing,
      strict,
      loose,
    });
  }
  out
}

/// Tracks that fit in the free stretches of a closed ring sampled every `step`.
fn runs(free: &[bool], step: f64, pitch: f64) -> usize {
  let n = free.len();
  if n == 0 {
    return 0;
  }
  if free.iter().all(|&f| f) {
    return ((n as f64 * step) / pitch + 1e-9).floor() as usize;
  }
  // Start just after a blocked point so no free stretch wraps past the start.
  let Some(start) = free.iter().position(|&f| !f) else {
    return 0;
  };
  let mut total = 0usize;
  let mut len: Option<usize> = None;
  for k in 1..=n {
    let f = free.get((start + k) % n).copied().unwrap_or(false);
    match (f, len) {
      (true, None) => len = Some(1),
      (true, Some(l)) => len = Some(l + 1),
      (false, Some(l)) => {
        total += (((l - 1) as f64 * step) / pitch + 1e-9).floor() as usize + 1;
        len = None;
      }
      (false, None) => {}
    }
  }
  if let Some(l) = len {
    total += (((l - 1) as f64 * step) / pitch + 1e-9).floor() as usize + 1;
  }
  total
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
  fn gap_formula_and_its_inverse() {
    let r = rules();
    // One track needs 2 * (0.0762 + 0.1524) = 0.4572 mm.
    assert_eq!(tracks_in_gap(0.4571, &r), 0);
    assert_eq!(tracks_in_gap(0.4572, &r), 1);
    // Each further track adds 0.3048 mm.
    assert_eq!(tracks_in_gap(0.4572 + 0.3048, &r), 2);
    assert_eq!(tracks_in_gap(0.4572 + 0.3048 - 0.001, &r), 1);
    for n in 1..8 {
      assert_eq!(tracks_in_gap(gap_for_tracks(n, &r), &r), n, "n = {n}");
      assert_eq!(tracks_in_gap(gap_for_tracks(n, &r) - 0.01, &r), n - 1);
    }
    assert_eq!(gap_for_tracks(0, &r), 0.0);
    // Two layers share the nets: 7 nets need room for 4 tracks.
    assert_eq!(gap_for_nets(7, 2, &r), gap_for_tracks(4, &r));
    assert_eq!(tracks_in_gap(f64::NAN, &r), 0);
  }

  #[test]
  fn runs_count_tracks_per_free_stretch() {
    // A ring of 40 points, 0.05 mm apart, with a wall leaving two stretches.
    let mut free = vec![false; 40];
    for f in free.iter_mut().take(10) {
      *f = true; // 10 points = 0.45 mm long: 0.45 / 0.3048 -> 1 + 1 tracks
    }
    for f in free.iter_mut().skip(20).take(3) {
      *f = true; // 3 points = 0.1 mm long: 1 track
    }
    assert_eq!(runs(&free, 0.05, 0.3048), 2 + 1);
    // Wrapping stretch is counted once, not twice.
    let mut wrap = vec![false; 40];
    for k in [38usize, 39, 0, 1] {
      wrap[k] = true;
    }
    assert_eq!(runs(&wrap, 0.05, 0.3048), 1);
    // No wall at all: the whole ring.
    assert_eq!(runs(&[true; 40], 0.05, 0.3048), 6);
    assert_eq!(runs(&[], 0.05, 0.3048), 0);
  }

  /// A 2 x 2 mm part with four nets that each have a partner pad 6 mm outside,
  /// and a ring of foreign pads at 1 mm that leaves one gap. Demand 4 nets;
  /// the gap holds fewer tracks, so the ring is over capacity.
  #[test]
  fn a_walled_ring_is_over_capacity() {
    let mut walls = String::new();
    // Wall pads along the top, bottom, left and right at distance 1 mm from the part box,
    // 0.5 mm wide with 0.5 mm gaps (centre-line room 0.5 - 0.457 = 0.043 mm: 1 track each gap).
    for k in -8..=8 {
      let x = k as f64 * 1.0;
      walls += &format!(
        r#"(pad "w" smd rect (at {x} -2.25) (size 0.5 0.5) (layers "F.Cu")) (pad "w" smd rect (at {x} 2.25) (size 0.5 0.5) (layers "F.Cu"))"#
      );
    }
    for k in -2..=2 {
      let y = k as f64 * 1.0;
      walls += &format!(
        r#"(pad "w" smd rect (at -2.25 {y}) (size 0.5 0.5) (layers "F.Cu")) (pad "w" smd rect (at 2.25 {y}) (size 0.5 0.5) (layers "F.Cu"))"#
      );
    }
    let src = format!(
      r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C") (net 4 "D") (net 9 "W")
  (gr_line (start -20 -20) (end 20 -20) (layer "Edge.Cuts"))
  (gr_line (start 20 -20) (end 20 20) (layer "Edge.Cuts"))
  (gr_line (start 20 20) (end -20 20) (layer "Edge.Cuts"))
  (gr_line (start -20 20) (end -20 -20) (layer "Edge.Cuts"))
  (footprint "P" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at -0.5 -0.5) (size 0.6 0.6) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 0.5 -0.5) (size 0.6 0.6) (layers "F.Cu") (net 2 "B"))
    (pad "3" smd rect (at -0.5 0.5) (size 0.6 0.6) (layers "F.Cu") (net 3 "C"))
    (pad "4" smd rect (at 0.5 0.5) (size 0.6 0.6) (layers "F.Cu") (net 4 "D")))
  (footprint "Q" (layer "F.Cu") (at 0 0) (property "Reference" "J1")
    (pad "1" smd rect (at -9 -9) (size 0.6 0.6) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 9 -9) (size 0.6 0.6) (layers "F.Cu") (net 2 "B"))
    (pad "3" smd rect (at -9 9) (size 0.6 0.6) (layers "F.Cu") (net 3 "C"))
    (pad "4" smd rect (at 9 9) (size 0.6 0.6) (layers "F.Cu") (net 4 "D")))
  (footprint "W" (layer "F.Cu") (at 0 0) (property "Reference" "W1") {walls})
)"#
    );
    let doc = autolayout_sexpr::parse(&src).unwrap();
    let board = Board::read(&doc).unwrap();
    let fi = board.footprints.iter().position(|f| f.reference == "U1").unwrap();
    let outline = board.outline;
    let layers = vec!["F.Cu".to_string()];
    let r = ring_reports(&board, fi, &[1.0, 6.0], &rules(), &layers, &[], outline);
    assert_eq!(r.len(), 2);
    // Ring at 1.0: the four nets cross it; the wall leaves a track or so per gap, 4 sides.
    assert_eq!(r[0].demand(), 4, "{:?}", r[0]);
    // The open ring far out (6 mm) is not walled: plenty of room.
    assert!(r[1].capacity_strict() > 4, "{:?}", r[1]);
    assert_eq!(r[1].demand(), 4);
    // Strict never exceeds loose.
    for rep in &r {
      assert!(rep.capacity_strict() <= rep.capacity_loose(), "{rep:?}");
    }
    // The walled ring holds far less than the open one.
    assert!(
      r[0].capacity_strict() < r[1].capacity_strict(),
      "{:?} {:?}",
      r[0],
      r[1]
    );
  }
}
