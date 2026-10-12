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

#[cfg(test)]
mod tests {
  use super::*;

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
