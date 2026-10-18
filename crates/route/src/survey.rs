//! Escape survey (M7.5, 2026-10-16): for each SMD pad, how far can a straight
//! track of the routing profile leave the pad along its axis before it comes
//! nearer than the clearance to a foreign pad? Continuous geometry, no grid.
//!
//! It answers one question: when the grid router cannot reach a pad, is the pad
//! really boxed in by fixed copper, or does the grid throw away room that
//! exists? Only pads count (the survey runs on stripped boards); tracks of
//! other nets are not consulted, so the answer is an upper bound on the room.

use autolayout_board::{Board, Point};

use crate::shape::{capsule_gap, pad_copper};

/// Search step along the axis (mm).
const STEP: f64 = 0.05;

/// The best straight escape of one pad.
#[derive(Debug, Clone, PartialEq)]
pub struct Room {
  /// (footprint index, pad index)
  pub pad: (usize, usize),
  /// Longest clear straight track from the pad edge (mm, capped at `max_len`).
  pub best: f64,
  /// Unit vector of that escape.
  pub dir: (f64, f64),
}

/// Best straight escape of pad (`fi`, `pi`): along its long axis away from the
/// middle of its part (both ways when it sits at the middle), or along either
/// axis for a squarish pad. `None` for pads that are not SMD, have no net, or
/// have no copper shape.
pub fn axis_room(
  board: &Board,
  track: f64,
  clearance: f64,
  margin: f64,
  fi: usize,
  pi: usize,
  max_len: f64,
) -> Option<Room> {
  let fp = board.footprints.get(fi)?;
  let pad = fp.pads.get(pi)?;
  if pad.kind != "smd" || pad.net.is_none() || !pad.at.x.is_finite() || !pad.at.y.is_finite() {
    return None;
  }
  // The size and angle, not `bbox()`: a rectangle's bbox is a square (see `shape::pad_extent`).
  let (w, h) = crate::shape::pad_extent(pad);
  let (hw, hh) = (w / 2.0, h / 2.0);
  let count = fp.pads.len().max(1) as f64;
  let (mx, my) = fp
    .pads
    .iter()
    .fold((0.0, 0.0), |(a, b), q| (a + q.at.x, b + q.at.y));
  let (rx, ry) = (pad.at.x - mx / count, pad.at.y - my / count);
  let outward = |ax: (f64, f64)| -> Vec<(f64, f64)> {
    let proj = rx * ax.0 + ry * ax.1;
    if proj.abs() > 0.05 {
      vec![(ax.0 * proj.signum(), ax.1 * proj.signum())]
    } else {
      vec![ax, (-ax.0, -ax.1)]
    }
  };
  let dirs: Vec<(f64, f64)> = if hw >= 1.5 * hh {
    outward((1.0, 0.0))
  } else if hh >= 1.5 * hw {
    outward((0.0, 1.0))
  } else {
    vec![(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
  };
  let layer = pad
    .layers
    .iter()
    .find(|l| l.as_str() == "F.Cu" || l.as_str() == "B.Cu")?;
  let need = clearance + margin;
  let others: Vec<_> = board
    .footprints
    .iter()
    .enumerate()
    .flat_map(|(qf, f)| f.pads.iter().enumerate().map(move |(qp, q)| (qf, qp, q)))
    .filter(|&(qf, qp, q)| {
      (qf, qp) != (fi, pi)
        && q.net != pad.net
        && q.kind != "np_thru_hole"
        && (q.at - pad.at).length() <= max_len + 4.0
        && q
          .layers
          .iter()
          .any(|l| l == layer || l == "*.Cu" || (l == "F&B.Cu"))
    })
    .map(|(_, _, q)| (pad_copper(q), q.clearance))
    .collect();
  let mut best: Option<Room> = None;
  for u in dirs {
    let along = if u.0.abs() > 0.5 { hw } else { hh };
    let p0 = Point::new(pad.at.x + u.0 * along, pad.at.y + u.1 * along);
    let mut len = 0.0;
    let mut t = 0.0;
    while t <= max_len + 1e-9 {
      let p1 = Point::new(p0.x + u.0 * t, p0.y + u.1 * t);
      let clear = others
        .iter()
        .all(|(s, c)| capsule_gap(p0, p1, track / 2.0, s) >= need.max(c.map_or(0.0, |o| o + margin)));
      if !clear {
        break;
      }
      len = t;
      t += STEP;
    }
    if best.as_ref().is_none_or(|b| len > b.best) {
      best = Some(Room {
        pad: (fi, pi),
        best: len,
        dir: u,
      });
    }
  }
  best
}
