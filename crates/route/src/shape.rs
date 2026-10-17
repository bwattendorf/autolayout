//! Copper shapes with a signed distance (negative inside), in board
//! millimetres. Used by the routing model (to rasterise obstacles) and by the
//! file verifier (to judge written copper), so both see a pad the same way.
//!
//! A pad is drawn two ways on purpose. As an **obstacle** a `roundrect` is
//! its full rectangle: other nets stay clear of the corners too, which is
//! conservative. As a place to **land** a track it is the true rounded
//! rectangle, because a track end in a rounded-off corner touches no copper
//! (found 2026-10-13: KiCad reported two dangling tracks ending 0.02–0.03 mm
//! inside the square corner of a roundrect pad that the router had treated as
//! square). Rounding follows the pad's `roundrect_rratio` (radius = ratio ×
//! the shorter side); a roundrect that does not state one is taken at the
//! largest legal ratio, 0.5, so the landing area is never overstated.

use autolayout_board::{BBox, Pad, Point};

/// Largest legal corner ratio; used when a roundrect pad states none.
pub const MAX_RRATIO: f64 = 0.5;

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
  /// Oriented rounded rectangle: centre, half extents, corner radius, angle
  /// (degrees, board frame). Radius 0 is a plain rectangle.
  RoundRect(Point, Point, f64, f64),
  /// Segment with a round end of radius `r` (circles are zero-length).
  Capsule(Point, Point, f64),
  Polygon(Vec<Point>),
  /// Union of shapes (a `custom` pad: its anchor plus its polygon primitives).
  Union(Vec<Shape>),
}

impl Shape {
  /// Signed distance from `p` to the shape's edge: negative inside.
  pub fn sdf(&self, p: Point) -> f64 {
    match self {
      Shape::RoundRect(c, half, r, angle) => {
        let l = (p - *c).rotated(-angle);
        let qx = l.x.abs() - (half.x - r);
        let qy = l.y.abs() - (half.y - r);
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
      }
      Shape::Capsule(a, b, r) => seg_dist(p, *a, *b) - r,
      Shape::Union(parts) => parts.iter().map(|q| q.sdf(p)).fold(f64::INFINITY, f64::min),
      Shape::Polygon(pts) => {
        let mut d = f64::INFINITY;
        for i in 0..pts.len() {
          if let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % pts.len())) {
            d = d.min(seg_dist(p, a, b));
          }
        }
        if polygon_contains(pts, p) { -d } else { d }
      }
    }
  }

  /// Distance from `p` to the shape, 0 inside.
  pub fn distance(&self, p: Point) -> f64 {
    self.sdf(p).max(0.0)
  }

  pub fn bbox(&self) -> Option<BBox> {
    match self {
      Shape::RoundRect(c, half, _, _) => {
        let r = half.x.hypot(half.y);
        Some(BBox {
          min: Point::new(c.x - r, c.y - r),
          max: Point::new(c.x + r, c.y + r),
        })
      }
      Shape::Capsule(a, b, r) => Some(BBox {
        min: Point::new(a.x.min(b.x) - r, a.y.min(b.y) - r),
        max: Point::new(a.x.max(b.x) + r, a.y.max(b.y) + r),
      }),
      Shape::Polygon(pts) => BBox::from_points(pts.iter().copied()),
      Shape::Union(parts) => parts
        .iter()
        .filter_map(Shape::bbox)
        .fold(None, |acc, b| BBox::union(acc, Some(b))),
    }
  }
}

pub fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
  let ab = b - a;
  let l2 = ab.x * ab.x + ab.y * ab.y;
  let t = if l2 == 0.0 {
    0.0
  } else {
    (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / l2).clamp(0.0, 1.0)
  };
  (p - Point::new(a.x + ab.x * t, a.y + ab.y * t)).length()
}

/// Even-odd point-in-polygon. KiCad writes a zone fill with holes as one ring
/// that visits each hole through a zero-width slit, which this handles.
pub fn polygon_contains(pts: &[Point], p: Point) -> bool {
  let mut c = false;
  let mut j = pts.len().saturating_sub(1);
  for i in 0..pts.len() {
    if let (Some(a), Some(b)) = (pts.get(i), pts.get(j))
      && (a.y > p.y) != (b.y > p.y)
      && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x
    {
      c = !c;
    }
    j = i;
  }
  c
}

fn capsule(pad: &Pad) -> Shape {
  let half = Point::new(pad.size.x / 2.0, pad.size.y / 2.0);
  let r = half.x.min(half.y);
  let along = Point::new((half.x - r).max(0.0), (half.y - r).max(0.0));
  Shape::Capsule(
    pad.at + along.rotated(pad.angle),
    pad.at - along.rotated(pad.angle),
    r,
  )
}

/// A `custom` pad: its anchor (circle or rectangle, `size`) plus every
/// `gr_poly` primitive, each filled polygon and its stroke, rotated with the pad.
/// Other primitive kinds are counted in `Pad::unmodelled_primitives`, not drawn.
fn custom(pad: &Pad, anchor_round: bool) -> Shape {
  let half = Point::new(pad.size.x / 2.0, pad.size.y / 2.0);
  let mut parts = vec![if anchor_round {
    Shape::Capsule(pad.at, pad.at, half.x.min(half.y))
  } else {
    Shape::RoundRect(pad.at, half, 0.0, pad.angle)
  }];
  for prim in &pad.primitives {
    let pts: Vec<Point> = prim.pts.iter().map(|q| pad.at + q.rotated(pad.angle)).collect();
    if prim.fill && pts.len() >= 3 {
      parts.push(Shape::Polygon(pts.clone()));
    }
    if prim.width > 0.0 {
      for i in 0..pts.len() {
        if let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % pts.len())) {
          parts.push(Shape::Capsule(a, b, prim.width / 2.0));
        }
      }
    }
  }
  Shape::Union(parts)
}

/// The pad as something to keep clear of: roundrects are their full rectangle.
pub fn pad_obstacle(pad: &Pad) -> Shape {
  match pad.shape.as_str() {
    "custom" => custom(pad, !pad.anchor_rect),
    "circle" | "oval" => capsule(pad),
    _ => Shape::RoundRect(
      pad.at,
      Point::new(pad.size.x / 2.0, pad.size.y / 2.0),
      0.0,
      pad.angle,
    ),
  }
}

/// The pad as copper a track can land on. `custom` pads use their anchor
/// rectangle (their primitives are not read), so are not exact.
pub fn pad_copper(pad: &Pad) -> Shape {
  match pad.shape.as_str() {
    "custom" => custom(pad, !pad.anchor_rect),
    "circle" | "oval" => capsule(pad),
    "roundrect" => {
      let half = Point::new(pad.size.x / 2.0, pad.size.y / 2.0);
      let ratio = pad.roundrect_rratio.unwrap_or(MAX_RRATIO).clamp(0.0, MAX_RRATIO);
      Shape::RoundRect(pad.at, half, ratio * pad.size.x.min(pad.size.y), pad.angle)
    }
    _ => Shape::RoundRect(
      pad.at,
      Point::new(pad.size.x / 2.0, pad.size.y / 2.0),
      0.0,
      pad.angle,
    ),
  }
}

/// Width and height of the pad's copper along the board axes. `Shape::bbox` is a
/// bounding square for a rectangle (it bounds the rotated shape's circle), so it
/// cannot tell a long pad from a square one; this uses the size and angle.
pub fn pad_extent(pad: &Pad) -> (f64, f64) {
  match pad.shape.as_str() {
    "custom" => pad_copper(pad)
      .bbox()
      .map_or((pad.size.x, pad.size.y), |b| (b.width(), b.height())),
    _ => {
      let (s, c) = pad.angle.to_radians().sin_cos();
      let (hx, hy) = (pad.size.x / 2.0, pad.size.y / 2.0);
      (
        2.0 * (hx * c.abs() + hy * s.abs()),
        2.0 * (hx * s.abs() + hy * c.abs()),
      )
    }
  }
}

/// Smallest signed distance from the segment `a..b` to `s`, minus `r`:
/// the gap between a track of radius `r` and `s`. `s` is convex, so the
/// distance along the segment is convex and a ternary search finds the minimum.
pub fn capsule_gap(a: Point, b: Point, r: f64, s: &Shape) -> f64 {
  // A union is the nearer of its parts; each part is convex (polygon fills are
  // convex in the pads seen so far; a concave one would need splitting).
  if let Shape::Union(parts) = s {
    return parts
      .iter()
      .map(|q| capsule_gap(a, b, r, q))
      .fold(f64::INFINITY, f64::min);
  }
  let at = |t: f64| Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
  let (mut lo, mut hi) = (0.0, 1.0);
  for _ in 0..60 {
    let m1 = lo + (hi - lo) / 3.0;
    let m2 = hi - (hi - lo) / 3.0;
    if s.sdf(at(m1)) < s.sdf(at(m2)) {
      hi = m2;
    } else {
      lo = m1;
    }
  }
  s.sdf(at((lo + hi) / 2.0)).min(s.sdf(a)).min(s.sdf(b)) - r
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rounded_corner_is_outside_the_copper_but_inside_the_square() {
    // The C22.1 case: 3.5 x 1.6, ratio 0.25 -> radius 0.4. A point 0.03 in
    // from the right edge and 0.02 down from the top edge.
    let sq = Shape::RoundRect(Point::new(0.0, 0.0), Point::new(1.75, 0.8), 0.0, 0.0);
    let rr = Shape::RoundRect(Point::new(0.0, 0.0), Point::new(1.75, 0.8), 0.4, 0.0);
    let p = Point::new(1.75 - 0.03, -0.8 + 0.02);
    assert!(sq.sdf(p) < 0.0);
    assert!(rr.sdf(p) > 0.0, "{}", rr.sdf(p));
    // The centre and the middle of a long side are inside both.
    assert!(rr.sdf(Point::new(0.0, 0.0)) < 0.0 && rr.sdf(Point::new(1.7, 0.0)) < 0.0);
    // Along an edge the sdf is exact: 0.1 outside the right edge.
    assert!((rr.sdf(Point::new(1.85, 0.0)) - 0.1).abs() < 1e-12);
  }

  #[test]
  fn rotation_swaps_the_extents() {
    let r = Shape::RoundRect(Point::new(0.0, 0.0), Point::new(1.0, 0.25), 0.0, 90.0);
    assert!(r.sdf(Point::new(0.0, 0.9)) < 0.0);
    assert!(r.sdf(Point::new(0.9, 0.0)) > 0.0);
  }

  #[test]
  fn polygon_sdf_has_the_right_sign() {
    let p = Shape::Polygon(vec![
      Point::new(0.0, 0.0),
      Point::new(2.0, 0.0),
      Point::new(2.0, 2.0),
      Point::new(0.0, 2.0),
    ]);
    assert!((p.sdf(Point::new(1.0, 1.0)) + 1.0).abs() < 1e-12);
    assert!((p.sdf(Point::new(3.0, 1.0)) - 1.0).abs() < 1e-12);
  }

  /// KiCad writes a hole in a zone fill as a slit in one ring (m1 round-3 data).
  #[test]
  fn a_ring_with_a_slit_hole_excludes_the_hole() {
    let p = |x: f64, y: f64| Point::new(x, y);
    let ring = vec![
      p(0.0, 0.0),
      p(10.0, 0.0),
      p(10.0, 10.0),
      p(0.0, 10.0),
      p(0.0, 5.0),
      p(4.0, 5.0),
      p(4.0, 6.0),
      p(6.0, 6.0),
      p(6.0, 4.0),
      p(4.0, 4.0),
      p(4.0, 5.0),
      p(0.0, 5.0),
    ];
    assert!(polygon_contains(&ring, p(8.0, 8.0)));
    assert!(polygon_contains(&ring, p(2.0, 2.0)));
    assert!(!polygon_contains(&ring, p(5.0, 5.0)), "inside the hole");
    assert!(!polygon_contains(&ring, p(12.0, 5.0)));
  }
}
