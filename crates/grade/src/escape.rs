//! Escape diagnosis: which SMD pads cannot get a fan-out via.
//!
//! Freerouting's fan-out stage reports only a total ("escaped pins: 263/434")
//! and on m1 that total separated two placements as clearly as a 75-minute
//! full route did (ROADMAP 2026-10-12). It does not say WHICH pins failed, so
//! a placer change cannot be aimed. This measures it per pad, from geometry:
//!
//! A pad *escapes* when some point within `reach` of its edge can hold a via
//! of diameter `via` with `clearance` to every pad of a different net (pads
//! of the same net may touch it), to the board edge, and to every via already
//! placed for another net. Pads are their real rotated rectangles.
//!
//! **Vias compete.** The first version checked each pad alone and found a
//! site for 99.8–100 % of pads on every m1 placement: it could not tell ours
//! (router: 59.7 % escaped) from the service's (89.9 %). The router's own
//! failures said why. On our board 67 of the non-MCU unconnected items were
//! GND/+3V3 pins (the service's: 21), pins that need only a short stub and a
//! via, failing where many sit together and their vias contend for the same
//! few spots. So pads claim via sites one at a time, in a fixed order, and
//! every via keeps `via + clearance` from the vias of other nets.
//!
//! This is a proxy for the router's fan-out, not the router. It is validated,
//! as every proxy here must be, by whether it ranks boards the way the router
//! did — and reported as unvalidated until it does.
//!
//! **STATUS: NOT VALIDATED — do not tune a placer on it.** Measured
//! 2026-10-13 on five m1 placements against Freerouting's fan-out:
//!
//! | placement | this proxy | router escaped |
//! |---|---|---|
//! | M4.1 packed | 91.0 % | 59.7 % |
//! | service's | 97.7 % | 89.9 % |
//! | + halo | 98.6 % | 64.3 % |
//! | + halo + spread | 98.6 % | 61.8 % |
//! | + halo, front-only, pairs | 98.2 % | 60.4 % |
//!
//! It orders the first two correctly and puts the three halo placements
//! above the service's, which the router does not. So local via space, alone
//! or with neighbouring vias competing, is not what fails on those boards.

use autolayout_board::{BBox, Board, Point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EscapeRules {
  /// Via outer diameter, mm.
  pub via: f64,
  /// Copper clearance, mm.
  pub clearance: f64,
  /// How far from the pad edge a fan-out via may sit, mm.
  pub reach: f64,
}

/// The rules both m1 DSN exports carried (KiCad Default netclass): via
/// 0.6 mm, clearance 0.2 mm; reach 1.5 mm.
pub const DEFAULT_RULES: EscapeRules = EscapeRules {
  via: 0.6,
  clearance: 0.2,
  reach: 1.5,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Escape {
  pub smd_pads: usize,
  pub blocked: usize,
  /// (reference, blocked pads, SMD pads) for every part with a blocked pad,
  /// most blocked first.
  pub by_part: Vec<(String, usize, usize)>,
}

struct Rect {
  centre: Point,
  half: Point,
  angle: f64,
  net: Option<u32>,
}

impl Rect {
  /// Distance from `p` to this rectangle (0 inside).
  fn distance(&self, p: Point) -> f64 {
    let local = (p - self.centre).rotated(-self.angle);
    let dx = (local.x.abs() - self.half.x).max(0.0);
    let dy = (local.y.abs() - self.half.y).max(0.0);
    dx.hypot(dy)
  }
}

pub fn escape(board: &Board, rules: EscapeRules) -> Escape {
  let mut rects: Vec<Rect> = Vec::new();
  // (footprint, pad is a netted SMD pad)
  let mut owner: Vec<(usize, bool)> = Vec::new();
  for (f, fp) in board.footprints.iter().enumerate() {
    for pad in &fp.pads {
      rects.push(Rect {
        centre: pad.at,
        half: Point::new(pad.size.x / 2.0, pad.size.y / 2.0),
        angle: pad.angle,
        net: pad.net,
      });
      owner.push((f, pad.kind == "smd" && pad.net.is_some()));
    }
  }
  let edge = board.outline;
  let keep = rules.via / 2.0 + rules.clearance;
  let via_gap = rules.via + rules.clearance;

  let fits = |p: Point, me: usize, net: Option<u32>, vias: &[(Point, Option<u32>)]| {
    if let Some(BBox { min, max }) = edge
      && (p.x - min.x < keep || max.x - p.x < keep || p.y - min.y < keep || max.y - p.y < keep)
    {
      return false;
    }
    vias
      .iter()
      .all(|&(v, vn)| (net.is_some() && vn == net) || (v - p).length() >= via_gap)
      && rects
        .iter()
        .enumerate()
        .all(|(i, r)| i == me || (net.is_some() && r.net == net) || r.distance(p) >= keep)
  };

  // Fixed order: by x, then y, so the result does not depend on file order.
  let mut order: Vec<usize> = (0..rects.len())
    .filter(|&i| owner.get(i).is_some_and(|o| o.1))
    .collect();
  order.sort_by(|&a, &b| {
    let (pa, pb) = (rects[a].centre, rects[b].centre);
    pa.x.total_cmp(&pb.x).then(pa.y.total_cmp(&pb.y)).then(a.cmp(&b))
  });

  let mut vias: Vec<(Point, Option<u32>)> = Vec::new();
  let mut per_part: Vec<(usize, usize)> = vec![(0, 0); board.footprints.len()];
  let (mut smd, mut blocked) = (0, 0);
  for i in order {
    let (Some(&(f, _)), Some(r)) = (owner.get(i), rects.get(i)) else {
      continue;
    };
    smd += 1;
    if let Some(c) = per_part.get_mut(f) {
      c.1 += 1;
    }
    let base = r.half.x.max(r.half.y);
    let mut site = None;
    'search: for k in 0..=((rules.reach / 0.1) as usize) {
      let radius = base + keep + k as f64 * 0.1;
      for a in 0..24 {
        let t = std::f64::consts::TAU * f64::from(a) / 24.0;
        let p = Point::new(r.centre.x + radius * t.cos(), r.centre.y + radius * t.sin());
        if fits(p, i, r.net, &vias) {
          site = Some(p);
          break 'search;
        }
      }
    }
    match site {
      Some(p) => vias.push((p, r.net)),
      None => {
        blocked += 1;
        if let Some(c) = per_part.get_mut(f) {
          c.0 += 1;
        }
      }
    }
  }
  let mut by_part: Vec<(String, usize, usize)> = per_part
    .iter()
    .enumerate()
    .filter(|(_, c)| c.0 > 0)
    .map(|(f, &(b, t))| {
      (
        board
          .footprints
          .get(f)
          .map_or_else(String::new, |fp| fp.reference.clone()),
        b,
        t,
      )
    })
    .collect();
  by_part.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
  Escape {
    smd_pads: smd,
    blocked,
    by_part,
  }
}
