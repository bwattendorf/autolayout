//! Minimal 2-D geometry in board millimetres, KiCad's y-down convention.

use std::ops::{Add, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
  pub x: f64,
  pub y: f64,
}

/// Axis-aligned box. Always `min <= max` on both axes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
  pub min: Point,
  pub max: Point,
}

impl Point {
  pub const fn new(x: f64, y: f64) -> Point {
    Point { x, y }
  }

  pub fn length(self) -> f64 {
    self.x.hypot(self.y)
  }

  /// Rotate by `degrees`, positive = counter-clockwise as seen on screen.
  /// With y pointing down that is `x' = x·cos + y·sin`, `y' = −x·sin + y·cos`.
  pub fn rotated(self, degrees: f64) -> Point {
    if degrees == 0.0 {
      return self;
    }
    let (s, c) = degrees.to_radians().sin_cos();
    Point::new(self.x * c + self.y * s, -self.x * s + self.y * c)
  }
}

impl Add for Point {
  type Output = Point;
  fn add(self, o: Point) -> Point {
    Point::new(self.x + o.x, self.y + o.y)
  }
}

impl Sub for Point {
  type Output = Point;
  fn sub(self, o: Point) -> Point {
    Point::new(self.x - o.x, self.y - o.y)
  }
}

impl BBox {
  /// Box around `points`; `None` for no points or any non-finite coordinate.
  pub fn from_points(points: impl IntoIterator<Item = Point>) -> Option<BBox> {
    let mut it = points.into_iter();
    let first = it.next()?;
    let mut b = BBox {
      min: first,
      max: first,
    };
    for p in std::iter::once(first).chain(it) {
      if !(p.x.is_finite() && p.y.is_finite()) {
        return None;
      }
      b.min = Point::new(b.min.x.min(p.x), b.min.y.min(p.y));
      b.max = Point::new(b.max.x.max(p.x), b.max.y.max(p.y));
    }
    Some(b)
  }

  pub fn union(a: Option<BBox>, b: Option<BBox>) -> Option<BBox> {
    match (a, b) {
      (Some(a), Some(b)) => BBox::from_points([a.min, a.max, b.min, b.max]),
      (a, None) => a,
      (None, b) => b,
    }
  }

  pub fn width(&self) -> f64 {
    self.max.x - self.min.x
  }

  pub fn height(&self) -> f64 {
    self.max.y - self.min.y
  }
}

impl Point {
  fn cross(self, o: Point) -> f64 {
    self.x * o.y - self.y * o.x
  }

  fn dot(self, o: Point) -> f64 {
    self.x * o.x + self.y * o.y
  }
}

/// A straight edge of an outline.
pub type Seg = (Point, Point);

/// Do two closed outlines, each given as a soup of edges, overlap with
/// positive area? True when an edge of one properly crosses an edge of the
/// other, or when a vertex (or the vertex centroid, which catches two
/// identical outlines stacked exactly) of one lies strictly inside the other,
/// further than `tol` from its boundary. Touching — shared edges, a vertex on
/// the other's boundary — is not overlap.
///
/// Works on the real outline, concave or not. Convex stand-ins (boxes,
/// hulls) fill notches: an electrolytic can's courtyard is a cross, and a part
/// in the notch between its body and a pad tab is clear of it while any convex
/// shape around the can reports a collision (m1 R6/C22, see `tests`).
pub fn outlines_overlap(a: &[Seg], b: &[Seg], tol: f64) -> bool {
  if a.is_empty() || b.is_empty() {
    return false;
  }
  for &s in a {
    for &t in b {
      if proper_cross(s, t, tol) {
        return true;
      }
    }
  }
  // Vertices alone miss equal-width rectangles stacked with an overlap: their
  // sides are collinear (no proper crossing) and every corner sits on the
  // other's boundary. The overlapped edge's midpoint is strictly inside.
  let probes = |segs: &[Seg]| {
    let mut pts: Vec<Point> = segs
      .iter()
      .flat_map(|&(p, q)| [p, q, Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0)])
      .collect();
    let n = pts.len() as f64;
    let sum = pts.iter().fold(Point::default(), |acc, &p| acc + p);
    pts.push(Point::new(sum.x / n, sum.y / n));
    pts
  };
  probes(a).iter().any(|&p| strictly_inside(p, b, tol))
    || probes(b).iter().any(|&p| strictly_inside(p, a, tol))
}

/// Are two outlines closer than `gap` — overlapping, or with some edge of one
/// within `gap` of some edge of the other? `gap <= 0` reduces to
/// [`outlines_overlap`].
pub fn outlines_within(a: &[Seg], b: &[Seg], gap: f64) -> bool {
  if outlines_overlap(a, b, 1e-3) {
    return true;
  }
  if gap <= 0.0 {
    return false;
  }
  a.iter().any(|&s| b.iter().any(|&t| seg_distance(s, t) < gap))
}

/// Shortest distance between two segments (zero if they cross).
pub fn seg_distance((a, b): Seg, (c, d): Seg) -> f64 {
  if proper_cross((a, b), (c, d), 0.0) {
    return 0.0;
  }
  dist_to_segment(a, c, d)
    .min(dist_to_segment(b, c, d))
    .min(dist_to_segment(c, a, b))
    .min(dist_to_segment(d, a, b))
}

fn orient(a: Point, b: Point, c: Point) -> f64 {
  (b - a).cross(c - a)
}

/// The two segments cross at a single interior point of both, each passing
/// clearly (by more than `tol`) to both sides of the other.
fn proper_cross((a, b): Seg, (c, d): Seg, tol: f64) -> bool {
  let side = |p: Point, q: Point, r: Point| {
    let len = (q - p).length();
    if len < 1e-12 { 0.0 } else { orient(p, q, r) / len }
  };
  let (d1, d2) = (side(a, b, c), side(a, b, d));
  let (d3, d4) = (side(c, d, a), side(c, d, b));
  ((d1 > tol && d2 < -tol) || (d1 < -tol && d2 > tol)) && ((d3 > tol && d4 < -tol) || (d3 < -tol && d4 > tol))
}

/// Even-odd ray cast towards +x, and further than `tol` from every edge.
fn strictly_inside(p: Point, segs: &[Seg], tol: f64) -> bool {
  let mut inside = false;
  for &(a, b) in segs {
    if dist_to_segment(p, a, b) <= tol {
      return false;
    }
    if (a.y > p.y) != (b.y > p.y) {
      let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
      if x > p.x {
        inside = !inside;
      }
    }
  }
  inside
}

fn dist_to_segment(p: Point, a: Point, b: Point) -> f64 {
  let ab = b - a;
  let len2 = ab.dot(ab);
  if len2 < 1e-24 {
    return (p - a).length();
  }
  let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
  (p - Point::new(a.x + ab.x * t, a.y + ab.y * t)).length()
}

/// Points along the circular arc through `start`, `mid`, `end`, in that
/// order, `segments` chords. Collinear input comes back as the three points.
pub fn arc_points(start: Point, mid: Point, end: Point, segments: usize) -> Vec<Point> {
  let Some(c) = circumcentre(start, mid, end) else {
    return vec![start, mid, end];
  };
  let r = (start - c).length();
  let ang = |p: Point| (p.y - c.y).atan2(p.x - c.x);
  let (a0, am, a2) = (ang(start), ang(mid), ang(end));
  let tau = std::f64::consts::TAU;
  // Sweep from start to end the way that passes through mid.
  let ccw = (a2 - a0).rem_euclid(tau);
  let to_mid = (am - a0).rem_euclid(tau);
  let sweep = if to_mid <= ccw { ccw } else { ccw - tau };
  let n = segments.max(2);
  (0..=n)
    .map(|i| {
      let a = a0 + sweep * i as f64 / n as f64;
      Point::new(c.x + r * a.cos(), c.y + r * a.sin())
    })
    .collect()
}

/// Points around a full circle.
pub fn circle_points(centre: Point, radius: f64, segments: usize) -> Vec<Point> {
  let n = segments.max(3);
  (0..n)
    .map(|i| {
      let a = std::f64::consts::TAU * i as f64 / n as f64;
      Point::new(centre.x + radius * a.cos(), centre.y + radius * a.sin())
    })
    .collect()
}

fn circumcentre(a: Point, b: Point, c: Point) -> Option<Point> {
  let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
  if d.abs() < 1e-12 {
    return None;
  }
  let sq = |p: Point| p.x * p.x + p.y * p.y;
  let ux = (sq(a) * (b.y - c.y) + sq(b) * (c.y - a.y) + sq(c) * (a.y - b.y)) / d;
  let uy = (sq(a) * (c.x - b.x) + sq(b) * (a.x - c.x) + sq(c) * (b.x - a.x)) / d;
  let p = Point::new(ux, uy);
  (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

#[cfg(test)]
mod tests {
  use super::*;

  const TOL: f64 = 1e-3;

  fn ring(pts: &[(f64, f64)]) -> Vec<Seg> {
    let p: Vec<Point> = pts.iter().map(|&(x, y)| Point::new(x, y)).collect();
    p.iter()
      .zip(p.iter().cycle().skip(1))
      .map(|(&a, &b)| (a, b))
      .collect()
  }

  fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Seg> {
    ring(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
  }

  mod outlines {
    use super::*;

    /// The real m1 case, in C22's frame. C22 (Capacitor_SMD:CP_Elec_6.3x7.7)
    /// has a cross-shaped courtyard: body to ±3.55 with chamfered left
    /// corners, pad tabs to ±4.7 only within ±1.05 in y (copied from the
    /// board's F.CrtYd lines). R6 (0402 at 90°) sits at (+4.51, −2.46) from
    /// C22's origin, in the notch between body and tab. KiCad's DRC reports
    /// no overlap; a box or a convex hull around C22 reports one.
    #[test]
    fn part_in_a_concave_notch_is_clear() {
      let c22 = ring(&[
        (-2.4, -3.55),
        (3.55, -3.55),
        (3.55, -1.05),
        (4.7, -1.05),
        (4.7, 1.05),
        (3.55, 1.05),
        (3.55, 3.55),
        (-2.4, 3.55),
        (-3.55, 2.4),
        (-3.55, 1.05),
        (-4.7, 1.05),
        (-4.7, -1.05),
        (-3.55, -1.05),
        (-3.55, -2.4),
      ]);
      let (cx, cy) = (4.512215, -2.458846);
      let r6 = rect(cx - 0.47, cy - 0.93, cx + 0.47, cy + 0.93);
      assert!(!outlines_overlap(&c22, &r6, TOL));
      // Pushed 0.5 mm down into the tab band, it does overlap.
      let r6_low = rect(cx - 0.47, cy - 0.43, cx + 0.47, cy + 1.43);
      assert!(outlines_overlap(&c22, &r6_low, TOL));
    }

    #[test]
    fn within_a_gap() {
      let a = rect(0.0, 0.0, 1.0, 1.0);
      assert!(
        outlines_within(&a, &rect(1.4, 0.0, 2.0, 1.0), 0.5),
        "0.4 apart, gap 0.5"
      );
      assert!(
        !outlines_within(&a, &rect(1.6, 0.0, 2.0, 1.0), 0.5),
        "0.6 apart, gap 0.5"
      );
      assert!(
        outlines_within(&a, &rect(0.5, 0.5, 2.0, 2.0), 0.0),
        "overlap is within any gap"
      );
      assert!(
        (seg_distance(
          (Point::new(0.0, 0.0), Point::new(1.0, 0.0)),
          (Point::new(0.5, 2.0), Point::new(0.5, 1.0))
        ) - 1.0)
          .abs()
          < 1e-12
      );
    }

    #[test]
    fn crossing_nested_touching_and_stacked() {
      let a = rect(0.0, 0.0, 2.0, 2.0);
      assert!(
        outlines_overlap(&a, &rect(1.0, 1.0, 3.0, 3.0), TOL),
        "corners crossing"
      );
      assert!(
        outlines_overlap(&a, &rect(0.5, 0.5, 1.5, 1.5), TOL),
        "one inside the other"
      );
      assert!(outlines_overlap(&a, &a.clone(), TOL), "stacked exactly");
      assert!(
        outlines_overlap(&a, &rect(-1.0, 0.5, 3.0, 1.5), TOL),
        "a bar straight through"
      );
      assert!(
        outlines_overlap(&a, &rect(0.0, 1.5, 2.0, 3.5), TOL),
        "equal-width blocks stacked 0.5 mm into each other (ganged terminal blocks)"
      );
      assert!(
        !outlines_overlap(&a, &rect(0.0, 2.0, 2.0, 4.0), TOL),
        "equal-width blocks abutting"
      );
      assert!(
        !outlines_overlap(&a, &rect(2.0, 0.0, 4.0, 2.0), TOL),
        "sharing an edge"
      );
      assert!(
        !outlines_overlap(&a, &rect(2.0, 2.0, 3.0, 3.0), TOL),
        "sharing a corner"
      );
      assert!(!outlines_overlap(&a, &rect(5.0, 5.0, 6.0, 6.0), TOL), "apart");
      assert!(!outlines_overlap(&a, &[], TOL), "no courtyard never overlaps");
    }
  }

  #[test]
  fn arc_passes_through_its_mid_point_and_bulges() {
    // Quarter circle of radius 1 from (1,0) through (√½,√½) to (0,1).
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let pts = arc_points(Point::new(1.0, 0.0), Point::new(h, h), Point::new(0.0, 1.0), 8);
    assert!(pts.iter().all(|p| (p.length() - 1.0).abs() < 1e-9));
    assert!(
      pts.iter().all(|p| p.x >= -1e-9 && p.y >= -1e-9),
      "took the long way round"
    );
  }

  fn close(a: Point, b: Point) -> bool {
    (a - b).length() < 1e-9
  }

  #[test]
  fn rotation_is_counter_clockwise_on_screen() {
    // +x rotated 90° CCW on a y-down screen points up, i.e. −y.
    assert!(close(Point::new(1.0, 0.0).rotated(90.0), Point::new(0.0, -1.0)));
    assert!(close(Point::new(1.0, 0.0).rotated(180.0), Point::new(-1.0, 0.0)));
    assert!(close(Point::new(0.0, 1.0).rotated(90.0), Point::new(1.0, 0.0)));
  }

  #[test]
  fn bbox_rejects_non_finite() {
    assert!(BBox::from_points([Point::new(0.0, 0.0), Point::new(f64::NAN, 1.0)]).is_none());
    assert!(BBox::from_points(std::iter::empty()).is_none());
  }
}
