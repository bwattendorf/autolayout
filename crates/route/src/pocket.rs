//! Plane pockets: where a poured plane cannot reach a pad or via of its own
//! net. The verifier of a stripped board has no zone fill to read, so a plane
//! is assumed solid (see `VerifyReport::plane_items_assumed`); on a dense via
//! field the antipads of other nets' vias and pads can cut the copper into
//! pieces, and the pieces holding no tap, or holding only some, are islands
//! (KiCad found these on boards r and c4/3p5 where the verifier said 0).
//!
//! This is an EMULATION of a fill, not KiCad's: free copper is a raster of
//! `RES` mm cells; a cell is blocked when it lies closer than the fill's
//! clearance to copper of another net (or to a hole), or outside the board
//! outline inset. Thin necks are removed by one erosion/dilation of half the
//! zone's minimum thickness. Taps (pads and vias of the plane's net) join the
//! components their copper touches; every tap not in the largest group is an
//! island. Its numbers are only as good as the match with KiCad's fill, which
//! is why the report carries them apart from `unconnected` until a Windows
//! round has compared them (`tools/zone_probe.py` for the comparison).

use crate::shape::Shape;
use autolayout_board::Point;

/// Raster cell, mm.
pub const RES: f64 = 0.05;
/// A tap touches fill when an opened free cell centre lies within this many
/// cells of its copper. A thin sliver between the tap and the fill is removed
/// by KiCad's minimum width, leaving the tap cut off (c4/3p5, round 11: a
/// 0.25 mm via between two foreign vias 0.409 mm either side), so this must be
/// close to contact, not a generous reach.
const TOUCH: f64 = 0.75;
/// Cells above this count make the emulation decline (a huge board at fine
/// resolution) instead of allocating without bound.
const MAX_CELLS: usize = 40_000_000;

/// Something the fill keeps `margin` clear of.
pub struct Block<'a> {
  pub shape: &'a Shape,
  pub margin: f64,
}

/// Settings of the pour being emulated.
pub struct Pour {
  pub min: Point,
  pub max: Point,
  /// Keep-out from the outline, mm (already includes the board's own edge rule).
  pub inset: f64,
  /// Zone minimum thickness, mm.
  pub min_thickness: f64,
}

/// What the emulation found.
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
  /// Indices of the taps NOT in the largest group of connected taps.
  pub islands: Vec<usize>,
  /// Per tap, the sizes (cells) of the free components its copper touches;
  /// empty when it touches none. For calibration against KiCad's fill.
  pub touched: Vec<Vec<usize>>,
  /// Cells in the main component (0 if there is none).
  pub main_cells: usize,
}

/// Indices of `taps` that are NOT in the largest group of connected taps, or
/// `None` when the emulation declined (board too large for the raster).
pub fn islands(pour: &Pour, blocks: &[Block], taps: &[&Shape]) -> Option<Vec<usize>> {
  analyse(pour, blocks, taps).map(|a| a.islands)
}

/// As [`islands`], with the numbers behind it.
pub fn analyse(pour: &Pour, blocks: &[Block], taps: &[&Shape]) -> Option<Analysis> {
  let (w, h) = (pour.max.x - pour.min.x, pour.max.y - pour.min.y);
  if !(w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
    return None;
  }
  let nx = (w / RES).ceil() as usize + 1;
  let ny = (h / RES).ceil() as usize + 1;
  if nx.checked_mul(ny)? > MAX_CELLS {
    return None;
  }
  let centre = |ix: usize, iy: usize| Point::new(pour.min.x + ix as f64 * RES, pour.min.y + iy as f64 * RES);
  let cell_range = |lo: f64, hi: f64, origin: f64, n: usize| {
    let a = (((lo - origin) / RES).floor().max(0.0)) as usize;
    let b = ((((hi - origin) / RES).ceil()).max(0.0) as usize).min(n.saturating_sub(1));
    (a.min(n.saturating_sub(1)), b)
  };
  let mut free = vec![true; nx * ny];
  // The outline inset.
  for iy in 0..ny {
    for ix in 0..nx {
      let c = centre(ix, iy);
      if c.x < pour.min.x + pour.inset
        || c.x > pour.max.x - pour.inset
        || c.y < pour.min.y + pour.inset
        || c.y > pour.max.y - pour.inset
      {
        free[iy * nx + ix] = false;
      }
    }
  }
  for b in blocks {
    let Some(bb) = b.shape.bbox() else { continue };
    let m = b.margin + RES;
    let (x0, x1) = cell_range(bb.min.x - m, bb.max.x + m, pour.min.x, nx);
    let (y0, y1) = cell_range(bb.min.y - m, bb.max.y + m, pour.min.y, ny);
    for iy in y0..=y1 {
      for ix in x0..=x1 {
        if let Some(f) = free.get_mut(iy * nx + ix)
          && *f
          && b.shape.sdf(centre(ix, iy)) < b.margin
        {
          *f = false;
        }
      }
    }
  }
  // Opening: erode then dilate by half the minimum thickness (in cells).
  let k = ((pour.min_thickness / 2.0) / RES).round().max(0.0) as usize;
  // `core` holds the cells whose whole `k`-cell neighbourhood is free: the
  // centres of the thickest part of the fill. A tap touches the opened fill
  // only if some core cell lies within `k` cells of its copper (the fill then
  // reaches it), not merely a cell the dilation re-added.
  let core = if k == 0 {
    free.clone()
  } else {
    erode(&free, nx, ny, k)
  };
  let free = if k == 0 { free } else { dilate(&core, nx, ny, k) };
  let reach = if k == 0 { RES * TOUCH } else { k as f64 * RES };
  // Components, 4-connected.
  let mut comp = vec![u32::MAX; nx * ny];
  let mut n_comp = 0u32;
  let mut stack: Vec<usize> = Vec::new();
  for start in 0..nx * ny {
    if !free.get(start).copied().unwrap_or(false) || comp.get(start).copied() != Some(u32::MAX) {
      continue;
    }
    if let Some(c) = comp.get_mut(start) {
      *c = n_comp;
    }
    stack.push(start);
    while let Some(i) = stack.pop() {
      let (ix, iy) = (i % nx, i / nx);
      let mut visit = |j: usize| {
        if free.get(j).copied().unwrap_or(false)
          && let Some(c) = comp.get_mut(j)
          && *c == u32::MAX
        {
          *c = n_comp;
          stack.push(j);
        }
      };
      if ix > 0 {
        visit(i - 1);
      }
      if ix + 1 < nx {
        visit(i + 1);
      }
      if iy > 0 {
        visit(i - nx);
      }
      if iy + 1 < ny {
        visit(i + nx);
      }
    }
    n_comp += 1;
  }
  // Which components each tap's copper touches (a cell within one raster step
  // of the tap's outline, so a pad smaller than a cell still reaches fill).
  let mut touches: Vec<Vec<u32>> = Vec::with_capacity(taps.len());
  for t in taps {
    let mut v: Vec<u32> = Vec::new();
    if let Some(bb) = t.bbox() {
      let (x0, x1) = cell_range(bb.min.x - 2.0 * RES, bb.max.x + 2.0 * RES, pour.min.x, nx);
      let (y0, y1) = cell_range(bb.min.y - 2.0 * RES, bb.max.y + 2.0 * RES, pour.min.y, ny);
      for iy in y0..=y1 {
        for ix in x0..=x1 {
          if let Some(&c) = comp.get(iy * nx + ix)
            && c != u32::MAX
            && core.get(iy * nx + ix) == Some(&true)
            && t.sdf(centre(ix, iy)) <= reach
            && !v.contains(&c)
          {
            v.push(c);
          }
        }
      }
    }
    touches.push(v);
  }
  // Groups of taps: two taps share a group when they touch a common component.
  // The largest group (most taps; ties to the lower component id) is the plane.
  let mut count = vec![0usize; n_comp as usize];
  for v in &touches {
    for &c in v {
      if let Some(n) = count.get_mut(c as usize) {
        *n += 1;
      }
    }
  }
  let main = count
    .iter()
    .enumerate()
    .max_by_key(|&(i, &n)| (n, std::cmp::Reverse(i)))
    .map(|(i, _)| i as u32);
  let mut sizes = vec![0usize; n_comp as usize];
  for &c in &comp {
    if let Some(n) = sizes.get_mut(c as usize) {
      *n += 1;
    }
  }
  Some(Analysis {
    islands: touches
      .iter()
      .enumerate()
      .filter(|(_, v)| main.is_none_or(|m| !v.contains(&m)))
      .map(|(i, _)| i)
      .collect(),
    touched: touches
      .iter()
      .map(|v| {
        v.iter()
          .map(|&c| sizes.get(c as usize).copied().unwrap_or(0))
          .collect()
      })
      .collect(),
    main_cells: main.and_then(|m| sizes.get(m as usize).copied()).unwrap_or(0),
  })
}

/// Offsets (dx, dy) inside a disc of `k` cells: KiCad's minimum-width pass
/// rounds by a circle, and a square would cut diagonal channels a circle keeps.
fn disc(k: usize) -> Vec<(i64, i64)> {
  let k = k as i64;
  let mut v = Vec::new();
  for dy in -k..=k {
    for dx in -k..=k {
      if dx * dx + dy * dy <= k * k {
        v.push((dx, dy));
      }
    }
  }
  v
}

/// Cells whose whole disc is free (cells outside the raster count as blocked).
fn erode(a: &[bool], nx: usize, ny: usize, k: usize) -> Vec<bool> {
  let d = disc(k);
  let at = |x: i64, y: i64| {
    x >= 0
      && y >= 0
      && (x as usize) < nx
      && (y as usize) < ny
      && a.get(y as usize * nx + x as usize) == Some(&true)
  };
  let mut out = vec![false; a.len()];
  for iy in 0..ny {
    for ix in 0..nx {
      if a.get(iy * nx + ix) == Some(&true)
        && d.iter().all(|&(dx, dy)| at(ix as i64 + dx, iy as i64 + dy))
        && let Some(o) = out.get_mut(iy * nx + ix)
      {
        *o = true;
      }
    }
  }
  out
}

/// Cells within a disc of `k` cells of a set cell.
fn dilate(a: &[bool], nx: usize, ny: usize, k: usize) -> Vec<bool> {
  let d = disc(k);
  let mut out = vec![false; a.len()];
  for iy in 0..ny {
    for ix in 0..nx {
      if a.get(iy * nx + ix) == Some(&true) {
        for &(dx, dy) in &d {
          let (x, y) = (ix as i64 + dx, iy as i64 + dy);
          if x >= 0
            && y >= 0
            && (x as usize) < nx
            && (y as usize) < ny
            && let Some(o) = out.get_mut(y as usize * nx + x as usize)
          {
            *o = true;
          }
        }
      }
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  fn circle(x: f64, y: f64, r: f64) -> Shape {
    Shape::Capsule(Point::new(x, y), Point::new(x, y), r)
  }
  fn pour() -> Pour {
    Pour {
      min: Point::new(0.0, 0.0),
      max: Point::new(20.0, 10.0),
      inset: 0.5,
      min_thickness: 0.2,
    }
  }

  #[test]
  fn an_open_plane_has_no_islands() {
    let (a, b) = (circle(3.0, 5.0, 0.3), circle(17.0, 5.0, 0.3));
    assert_eq!(islands(&pour(), &[], &[&a, &b]), Some(vec![]));
  }

  #[test]
  fn a_wall_of_antipads_splits_the_plane_and_the_small_side_is_an_island() {
    // A column of foreign vias, 1.0 mm round with 0.25 clearance, every 0.5 mm
    // from the top inset to the bottom inset, walls the right side off.
    let wall: Vec<Shape> = (0..=20)
      .map(|i| circle(10.0, 0.5 + i as f64 * 0.45, 0.5))
      .collect();
    let blocks: Vec<Block> = wall
      .iter()
      .map(|s| Block {
        shape: s,
        margin: 0.25,
      })
      .collect();
    let (a, b, c) = (
      circle(3.0, 5.0, 0.3),
      circle(5.0, 3.0, 0.3),
      circle(17.0, 5.0, 0.3),
    );
    let isl = islands(&pour(), &blocks, &[&a, &b, &c]);
    assert_eq!(isl, Some(vec![2]));
  }

  #[test]
  fn a_tap_boxed_in_by_foreign_copper_is_an_island() {
    let ring = circle(5.0, 5.0, 1.0);
    let blocks = [Block {
      shape: &ring,
      margin: 0.25,
    }];
    // The tap sits on the ring's centre: every cell near it is blocked.
    let inner = circle(5.0, 5.0, 0.2);
    let outer = circle(15.0, 5.0, 0.3);
    let other = circle(16.0, 6.0, 0.3);
    assert_eq!(
      islands(&pour(), &blocks, &[&inner, &outer, &other]),
      Some(vec![0])
    );
  }

  /// c4/3p5mil-candidate3, round 11, KiCad: the +3V3 via at (132.0772, 73.0328)
  /// is cut off by two foreign 0.25 mm vias 0.409 mm either side of it (and
  /// three more further out). Coordinates are the file's.
  #[test]
  fn a_via_squeezed_between_two_foreign_vias_is_an_island() {
    let foreign: Vec<Shape> = [
      (132.6256, 74.1296),
      (132.8084, 71.936),
      (132.4428, 72.85),
      (130.2492, 72.4844),
      (131.7116, 73.2156),
    ]
    .iter()
    .map(|&(x, y)| circle(x, y, 0.125))
    .collect();
    let blocks: Vec<Block> = foreign
      .iter()
      .map(|s| Block {
        shape: s,
        margin: 0.25,
      })
      .collect();
    let tap = circle(132.0772, 73.0328, 0.125);
    let far = circle(125.0, 80.0, 0.125);
    let far2 = circle(126.0, 80.0, 0.125);
    let p = Pour {
      min: Point::new(120.0, 68.0),
      max: Point::new(140.0, 84.0),
      inset: 0.5,
      min_thickness: 0.2,
    };
    let a = analyse(&p, &blocks, &[&tap, &far, &far2]).unwrap();
    assert_eq!(a.islands, vec![0], "{a:?}");
  }

  /// c2/3p5mil-candidate2, round 12: the +3V3 via at (136.0988, 73.0328) has a
  /// wall of antipads on one side and open copper on the other; KiCad reports
  /// it connected.
  #[test]
  fn a_via_at_the_edge_of_an_antipad_wall_is_not_an_island() {
    let (cx, cy) = (136.0988, 73.0328);
    let foreign: Vec<Shape> = [
      (1.280, 0.183),
      (0.366, 0.183),
      (-0.366, 0.183),
      (0.366, 0.731),
      (-0.731, -0.183),
    ]
    .iter()
    .map(|&(dx, dy)| circle(cx + dx, cy + dy, 0.125))
    .collect();
    let blocks: Vec<Block> = foreign
      .iter()
      .map(|s| Block {
        shape: s,
        margin: 0.25,
      })
      .collect();
    let tap = circle(cx, cy, 0.125);
    let far = circle(cx - 8.0, cy + 6.0, 0.125);
    let far2 = circle(cx - 7.0, cy + 6.0, 0.125);
    let p = Pour {
      min: Point::new(cx - 10.0, cy - 8.0),
      max: Point::new(cx + 10.0, cy + 8.0),
      inset: 0.5,
      min_thickness: 0.2,
    };
    let a = analyse(&p, &blocks, &[&tap, &far, &far2]).unwrap();
    assert!(a.islands.is_empty(), "{a:?}");
  }

  #[test]
  fn an_oversized_raster_declines() {
    let p = Pour {
      min: Point::new(0.0, 0.0),
      max: Point::new(1.0e5, 1.0e5),
      inset: 0.5,
      min_thickness: 0.2,
    };
    assert_eq!(islands(&p, &[], &[]), None);
  }
}
