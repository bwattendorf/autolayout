//! Judge a board's copper from the written file alone (replaces the old
//! `check`, which re-read the router's own grid and so shared its blind
//! spots: on 2026-10-13 it reported 0 problems on a board where KiCad found
//! 2 unconnected pads, 2 dangling tracks and 2 hole-to-hole violations).
//!
//! This reads segments, arcs and vias out of the file text and pads out of the
//! board model, and checks them in continuous geometry, never on the routing
//! grid. It re-implements, from the KiCad rules as the project states them,
//! the DRC categories that mattered so far:
//!
//! * **unconnected** — per net, components that hold a pad (or the net's
//!   plane) minus one. Items join the way KiCad joins them: a track *end
//!   point* inside a pad's copper, inside a via's disc, or on another track's
//!   end point (same layer, same net); a via centre inside a pad.
//! * **track_dangling** — a track end that joins nothing.
//! * **hole_to_hole** — via/pad drill edges nearer than `min_hole_to_hole`.
//! * **clearance** — copper of different nets nearer than the larger of the
//!   two classes' clearances, tracks and vias against everything.
//! * **copper_edge** — track/via copper nearer the board outline box than
//!   `min_copper_edge_clearance`; **track_width**, **via_size** minimums.
//!
//! Not covered: hole_clearance, zones' own fill and clearance, silk, courtyard,
//! anything about arcs' exact shape (an arc is a chain of chords). Pads are
//! exact (rounded rectangles, capsules); `custom` pads use their anchor.
//! **It is still our code.** The judge is `kicad-cli pcb drc`; this exists so
//! the tool stops reporting clean on a board KiCad rejects, and its
//! agreement with KiCad is itself measured (docs/m7-results).

use std::collections::HashMap;

use autolayout_board::{Board, Point, arc_points};
use autolayout_sexpr::{Document, List};

use crate::rules::DesignRules;
use crate::shape::{Shape, capsule_gap as gap, pad_copper};

const EPS: f64 = 1e-6;

/// KiCad joins a track end to a pad or via when the track's copper (its end
/// disc, radius = half the width) reaches the pad's polygon, which
/// approximates round corners to within the board's `max_error`. Calibrated on
/// the m1 fab board (2026-10-13, KiCad 9.0.1): of 35 track ends lying outside
/// the true rounded-rectangle copper, every end up to 0.0528 mm outside was
/// joined and the two at 0.0680 and 0.0687 mm were reported dangling. So the
/// threshold is half the track width plus this slack; it is measured on that
/// board, not taken from KiCad's source.
const LAND_SLACK: f64 = 0.01;

/// KiCad did not flag a 0.0001 mm clearance shortfall (m1, same run). Shortfalls
/// below this are not counted; it can hide a real violation of up to 1 um.
const CLEARANCE_SLACK: f64 = 0.001;

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
  pub kind: &'static str,
  pub at: Point,
  pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VerifyReport {
  pub segments: usize,
  pub vias: usize,
  pub unconnected: usize,
  pub dangling_tracks: usize,
  pub hole_to_hole: usize,
  pub clearance: usize,
  /// Copper of different nets overlapping (clearance below zero): KiCad's
  /// `shorting_items`, which it counts apart from `clearance`.
  pub shorting: usize,
  pub copper_edge: usize,
  pub track_width: usize,
  pub via_size: usize,
  /// Pads and vias joined to a declared plane without looking at any zone
  /// fill. The verifier does not read fills, so on these items it cannot see
  /// an island split off by antipads or tracks (KiCad did, on Freerouting's
  /// m1: 17 unconnected items the verifier missed). Not a finding; a limit.
  pub plane_items_assumed: usize,
  /// `filled_polygon` islands read from zones in the file; each is its own
  /// component until a same-net pad, via or track lies on it.
  pub fill_polygons: usize,
  pub issues: Vec<Issue>,
}

impl VerifyReport {
  pub fn clean(&self) -> bool {
    self.unconnected
      + self.dangling_tracks
      + self.hole_to_hole
      + self.clearance
      + self.copper_edge
      + self.track_width
      + self.via_size
      == 0
  }

  pub fn summary(&self) -> String {
    let mut s = format!(
      "{} segments, {} vias: unconnected {}, track_dangling {}, hole_to_hole {}, clearance {}, shorting {}, copper_edge {}, track_width {}, via_size {}",
      self.segments,
      self.vias,
      self.unconnected,
      self.dangling_tracks,
      self.hole_to_hole,
      self.clearance,
      self.shorting,
      self.copper_edge,
      self.track_width,
      self.via_size
    );
    if self.fill_polygons > 0 {
      s += &format!(
        "\n  zone fills read: {} polygons (each island is a component of its net)",
        self.fill_polygons
      );
    }
    if self.plane_items_assumed > 0 {
      s += &format!(
        "\n  LIMIT: {} pads/vias on plane nets were assumed connected to their plane because this file holds no zone fill for that layer and net (a stripped board); islands in a plane are NOT checked (unconnected 0 does not cover them)",
        self.plane_items_assumed
      );
    }
    s
  }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
  Pad,
  Track,
  Via,
}

struct Item {
  kind: Kind,
  net: u32,
  shape: Shape,
  layers: Vec<usize>,
  /// Drill: centre and diameter.
  hole: Option<(Point, f64)>,
  /// Track ends.
  ends: Option<(Point, Point)>,
  /// Pad or via centre: KiCad joins a track whose body passes over it.
  anchor: Option<Point>,
  /// Via/track diameter or width, for the minimum checks.
  size: f64,
}

struct Uf(Vec<usize>);

impl Uf {
  fn find(&mut self, mut x: usize) -> usize {
    while let Some(&p) = self.0.get(x) {
      if p == x {
        break;
      }
      let gp = self.0.get(p).copied().unwrap_or(p);
      if let Some(e) = self.0.get_mut(x) {
        *e = gp;
      }
      x = gp;
    }
    x
  }

  fn union(&mut self, a: usize, b: usize) {
    let (ra, rb) = (self.find(a), self.find(b));
    if let Some(e) = self.0.get_mut(ra) {
      *e = rb;
    }
  }
}

fn copper_layers(root: &List) -> Vec<String> {
  root
    .child("layers")
    .map(|l| {
      l.items
        .iter()
        .filter_map(|n| n.as_list())
        .filter_map(|e| e.atom(1))
        .filter(|n| n.ends_with(".Cu"))
        .map(|n| n.into_owned())
        .collect()
    })
    .unwrap_or_default()
}

fn xy(l: Option<&List>) -> Option<Point> {
  let l = l?;
  Some(Point::new(l.number(1)?, l.number(2)?))
}

fn resolve_layers(names: &[String], all: &[String]) -> Vec<usize> {
  let mut out = Vec::new();
  for n in names {
    for (i, l) in all.iter().enumerate() {
      let hit = match n.as_str() {
        "*.Cu" => true,
        "F&B.Cu" => l == "F.Cu" || l == "B.Cu",
        s => s == l,
      };
      if hit && !out.contains(&i) {
        out.push(i);
      }
    }
  }
  out
}

pub fn verify(doc: &Document, board: &Board, rules: &DesignRules, planes: &[(String, u32)]) -> VerifyReport {
  let mut rep = VerifyReport::default();
  let Some(root) = doc.root() else { return rep };
  let cu = copper_layers(root);
  let name = |n: u32| board.nets.get(&n).map(String::as_str).unwrap_or("");
  let mut items: Vec<Item> = Vec::new();

  for fp in &board.footprints {
    for pad in &fp.pads {
      if pad.kind == "np_thru_hole" {
        if let Some(d) = pad.drill {
          items.push(Item {
            kind: Kind::Pad,
            net: 0,
            shape: Shape::Capsule(pad.at, pad.at, 0.0),
            layers: Vec::new(),
            hole: Some((pad.at, d.x.max(d.y))),
            ends: None,
            anchor: None,
            size: 0.0,
          });
        }
        continue;
      }
      items.push(Item {
        kind: Kind::Pad,
        net: pad.net.unwrap_or(0),
        shape: pad_copper(pad),
        layers: resolve_layers(&pad.layers, &cu),
        hole: pad.drill.map(|d| (pad.at, d.x.max(d.y))),
        ends: None,
        anchor: Some(pad.at),
        size: 0.0,
      });
    }
  }
  let n_pads = items.len();
  let all_layers: Vec<usize> = (0..cu.len()).collect();

  for it in root.items.iter().filter_map(|n| n.as_list()) {
    match it.head().as_deref() {
      Some("segment") | Some("arc") => {
        let (Some(layer), Some(w)) = (
          it.child("layer").and_then(|l| l.atom(1)),
          it.child("width").and_then(|w| w.number(1)),
        ) else {
          continue;
        };
        let net = it.child("net").and_then(|n| n.number(1)).map_or(0, |n| n as u32);
        let li = resolve_layers(&[layer.into_owned()], &cu);
        let pts: Vec<Point> = if it.is("arc") {
          match (xy(it.child("start")), xy(it.child("mid")), xy(it.child("end"))) {
            (Some(a), Some(b), Some(c)) => arc_points(a, b, c, 16),
            _ => continue,
          }
        } else {
          match (xy(it.child("start")), xy(it.child("end"))) {
            (Some(a), Some(b)) => vec![a, b],
            _ => continue,
          }
        };
        rep.segments += 1;
        for pair in pts.windows(2) {
          if let [a, b] = pair {
            items.push(Item {
              kind: Kind::Track,
              net,
              shape: Shape::Capsule(*a, *b, w / 2.0),
              layers: li.clone(),
              hole: None,
              ends: Some((*a, *b)),
              anchor: None,
              size: w,
            });
          }
        }
      }
      Some("via") => {
        let (Some(at), Some(size)) = (xy(it.child("at")), it.child("size").and_then(|s| s.number(1))) else {
          continue;
        };
        let drill = it.child("drill").and_then(|d| d.number(1)).unwrap_or(0.0);
        let net = it.child("net").and_then(|n| n.number(1)).map_or(0, |n| n as u32);
        rep.vias += 1;
        items.push(Item {
          kind: Kind::Via,
          net,
          shape: Shape::Capsule(at, at, size / 2.0),
          layers: all_layers.clone(),
          hole: Some((at, drill)),
          ends: None,
          anchor: Some(at),
          size,
        });
      }
      _ => {}
    }
  }

  // Spatial hash of every item, bounding boxes grown by the largest clearance.
  let maxcl = rules
    .classes
    .iter()
    .map(|c| c.clearance)
    .fold(rules.min.clearance, f64::max)
    + 0.01;
  let bucket = 1.0;
  let cell = |v: f64| (v / bucket).floor() as i64;
  let mut hash: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
  for (i, it) in items.iter().enumerate() {
    if let Some(bb) = it.shape.bbox() {
      for x in cell(bb.min.x - maxcl)..=cell(bb.max.x + maxcl) {
        for y in cell(bb.min.y - maxcl)..=cell(bb.max.y + maxcl) {
          hash.entry((x, y)).or_default().push(i);
        }
      }
    }
  }
  let near = |i: usize| -> Vec<usize> {
    let mut v = Vec::new();
    if let Some(bb) = items.get(i).and_then(|it| it.shape.bbox()) {
      for x in cell(bb.min.x - maxcl)..=cell(bb.max.x + maxcl) {
        for y in cell(bb.min.y - maxcl)..=cell(bb.max.y + maxcl) {
          if let Some(b) = hash.get(&(x, y)) {
            v.extend(b.iter().copied().filter(|&j| j != i));
          }
        }
      }
    }
    v.sort_unstable();
    v.dedup();
    v
  };

  // Zone fills: every `filled_polygon` of a net zone is one island of copper.
  let mut fills: Vec<(u32, usize, Vec<Point>)> = Vec::new();
  for z in root.children("zone") {
    if z.child("keepout").is_some() {
      continue;
    }
    let net = z.child("net").and_then(|n| n.number(1)).map_or(0, |n| n as u32);
    if net == 0 {
      continue;
    }
    for f in z.children("filled_polygon") {
      let Some(li) = f
        .child("layer")
        .and_then(|l| l.atom(1))
        .and_then(|l| cu.iter().position(|c| *c == *l))
      else {
        continue;
      };
      let pts: Vec<Point> = f
        .child("pts")
        .map(|p| p.children("xy").filter_map(|l| xy(Some(l))).collect())
        .unwrap_or_default();
      if pts.len() >= 3 {
        fills.push((net, li, pts));
      }
    }
  }
  rep.fill_polygons = fills.len();
  let fill_base = items.len() + planes.len();
  let mut uf = Uf((0..fill_base + fills.len()).collect());
  let mut end_joined: Vec<(bool, bool)> = vec![(false, false); items.len()];
  let share = |a: &Item, b: &Item| a.layers.iter().any(|l| b.layers.contains(l));
  let mut issues: Vec<Issue> = Vec::new();

  for i in n_pads..items.len() {
    let Some(a) = items.get(i) else { continue };
    for j in near(i) {
      let Some(b) = items.get(j) else { continue };
      let mut clearance_check = true;
      // Connectivity (same net, nonzero).
      if a.net != 0 && a.net == b.net && share(a, b) {
        match (a.kind, b.kind) {
          (Kind::Track, Kind::Pad | Kind::Via) => {
            if let (Some((p, q)), Some(c)) = (a.ends, b.anchor)
              && crate::shape::seg_dist(c, p, q) <= a.size / 2.0 + LAND_SLACK
            {
              uf.union(i, j);
            }
            if let Some((p, q)) = a.ends {
              for (k, e) in [p, q].into_iter().enumerate() {
                if b.shape.sdf(e) <= a.size / 2.0 + LAND_SLACK {
                  uf.union(i, j);
                  if let Some(ej) = end_joined.get_mut(i) {
                    if k == 0 { ej.0 = true } else { ej.1 = true }
                  }
                }
              }
            }
          }
          (Kind::Via, Kind::Track) => {
            // A via dropped in the middle of a track joins it.
            if let (Some(c), Some((r, s))) = (a.anchor, b.ends)
              && crate::shape::seg_dist(c, r, s) <= b.size / 2.0 + LAND_SLACK
            {
              uf.union(i, j);
            }
          }
          (Kind::Track, Kind::Track) => {
            // A track end that lies on the other track (its end or its body)
            // joins it: the router branches nets from the middle of a run,
            // and KiCad did not report those as unconnected.
            if let (Some((p, q)), Some((r, s))) = (a.ends, b.ends) {
              for (k, e) in [p, q].into_iter().enumerate() {
                if crate::shape::seg_dist(e, r, s) <= b.size / 2.0 + EPS {
                  uf.union(i, j);
                  if let Some(ej) = end_joined.get_mut(i) {
                    if k == 0 { ej.0 = true } else { ej.1 = true }
                  }
                }
              }
            }
          }
          (Kind::Via, Kind::Pad) => {
            // The via's copper disc reaching the pad's copper joins them; a via
            // dropped on a pad's rounded corner still lands on the pad.
            if let Some((c, _)) = a.hole
              && b.shape.sdf(c) <= a.size / 2.0
            {
              uf.union(i, j);
            }
          }
          _ => {}
        }
      }
      if a.net == b.net && a.net != 0 {
        clearance_check = false; // same net never conflicts
      }
      // Hole to hole.
      if let (Some((ca, da)), Some((cb, db))) = (a.hole, b.hole)
        && (a.kind == Kind::Via || b.kind == Kind::Via)
        && (j > i || b.kind == Kind::Pad)
      {
        let edge = (ca - cb).length() - (da + db) / 2.0;
        if edge < rules.min.hole_to_hole - EPS {
          rep.hole_to_hole += 1;
          issues.push(Issue {
            kind: "hole_to_hole",
            at: ca,
            detail: format!(
              "holes {:.4} mm apart (edge to edge), rule {:.4}",
              edge, rules.min.hole_to_hole
            ),
          });
        }
      }
      // Clearance: only when this is the first time we see the pair.
      if clearance_check && (j > i || b.kind == Kind::Pad) && share(a, b) {
        let need = rules.clearance_between(name(a.net), name(b.net));
        let g = match &a.shape {
          Shape::Capsule(p, q, r) => gap(*p, *q, *r, &b.shape),
          other => b.shape.sdf(match other {
            Shape::RoundRect(c, ..) => *c,
            _ => Point::default(),
          }),
        };
        if g < 0.0 {
          rep.shorting += 1;
        } else if g < need - CLEARANCE_SLACK {
          rep.clearance += 1;
        }
        if g < need - CLEARANCE_SLACK {
          let at = match &a.shape {
            Shape::Capsule(p, ..) => *p,
            _ => Point::default(),
          };
          issues.push(Issue {
            kind: if g < 0.0 { "shorting_items" } else { "clearance" },
            at,
            detail: format!(
              "{} vs {}: {:.4} mm, rule {:.4}",
              name(a.net),
              name(b.net),
              g,
              need
            ),
          });
        }
      }
    }
  }

  // Fill islands: a same-net pad, via or track lying on an island joins it; a
  // track end inside one is not dangling. Sample points stand in for the exact
  // overlap: a pad's centre, corners and edge midpoints, a via's centre and a
  // ring inside its disc, a track every 0.05 mm along its centre line.
  for (k, (net, li, poly)) in fills.iter().enumerate() {
    let node = fill_base + k;
    for (i, it) in items.iter().enumerate() {
      if it.net != *net || !it.layers.contains(li) {
        continue;
      }
      let hit = |pt: Point| crate::shape::polygon_contains(poly, pt);
      let touches = match &it.shape {
        Shape::RoundRect(c, h, _, a) => {
          let mut pts = vec![*c];
          for (sx, sy) in [
            (-0.9, -0.9),
            (0.9, -0.9),
            (-0.9, 0.9),
            (0.9, 0.9),
            (0.0, -0.9),
            (0.0, 0.9),
            (-0.9, 0.0),
            (0.9, 0.0),
          ] {
            pts.push(*c + Point::new(h.x * sx, h.y * sy).rotated(*a));
          }
          pts.iter().any(|p| hit(*p))
        }
        Shape::Capsule(p, q, r) => {
          let len = (*q - *p).length();
          let n = ((len / 0.05).ceil() as usize).clamp(1, 400);
          let along = (0..=n).any(|t| {
            let f = t as f64 / n as f64;
            hit(Point::new(p.x + (q.x - p.x) * f, p.y + (q.y - p.y) * f))
          });
          along
            || (it.kind == Kind::Via
              && [(0.7, 0.0), (-0.7, 0.0), (0.0, 0.7), (0.0, -0.7)]
                .iter()
                .any(|(dx, dy)| hit(Point::new(p.x + r * dx, p.y + r * dy))))
        }
        Shape::Polygon(_) => false,
      };
      if touches {
        uf.union(i, node);
      }
      if it.kind == Kind::Track
        && let (Some((p, q)), Some(ej)) = (it.ends, end_joined.get_mut(i))
      {
        if hit(p) {
          ej.0 = true;
        }
        if hit(q) {
          ej.1 = true;
        }
      }
    }
  }

  let mut assumed_planes: Vec<usize> = Vec::new();
  // Plane nodes, only where the file holds no fill for that layer and net: a
  // via of the net, or a pad with copper on the plane layer, is ASSUMED joined.
  for (k, (layer, net)) in planes.iter().enumerate() {
    let node = items.len() + k;
    let Some(li) = cu.iter().position(|l| l == layer) else {
      continue;
    };
    if fills.iter().any(|(n, l, _)| n == net && *l == li) {
      continue;
    }
    assumed_planes.push(k);
    for (i, it) in items.iter().enumerate() {
      if it.net == *net && ((it.kind == Kind::Via) || (it.kind == Kind::Pad && it.layers.contains(&li))) {
        uf.union(i, node);
        rep.plane_items_assumed += 1;
      }
    }
  }

  // Dangling track ends and minimums.
  for (i, it) in items.iter().enumerate() {
    if it.kind == Kind::Track {
      let (e0, e1) = end_joined.get(i).copied().unwrap_or((true, true));
      if let Some((p, q)) = it.ends
        && (!e0 || !e1)
      {
        let at = if e0 { q } else { p };
        rep.dangling_tracks += 1;
        // How far outside the nearest copper of its own net the end lies.
        let nearest = items
          .iter()
          .filter(|o| {
            o.net == it.net && o.kind != Kind::Track && o.layers.iter().any(|l| it.layers.contains(l))
          })
          .map(|o| o.shape.sdf(at))
          .fold(f64::INFINITY, f64::min);
        issues.push(Issue {
          kind: "track_dangling",
          at,
          detail: format!(
            "net {}, {:.4} mm outside the nearest pad/via of its net",
            name(it.net),
            nearest
          ),
        });
      }
      if it.size < rules.min.track_width - EPS {
        rep.track_width += 1;
      }
      if let Shape::Capsule(p, q, r) = &it.shape
        && let Some(bb) = board.outline
      {
        let keep = rules.min.copper_edge_clearance;
        let near_edge = |pt: &Point| {
          pt.x - r - bb.min.x < keep - EPS
            || bb.max.x - pt.x - r < keep - EPS
            || pt.y - r - bb.min.y < keep - EPS
            || bb.max.y - pt.y - r < keep - EPS
        };
        if near_edge(p) || near_edge(q) {
          rep.copper_edge += 1;
          issues.push(Issue {
            kind: "copper_edge",
            at: *p,
            detail: format!("net {}", name(it.net)),
          });
        }
      }
    } else if it.kind == Kind::Via {
      if it.size < rules.min.via_diameter - EPS {
        rep.via_size += 1;
      }
      if let (Shape::Capsule(p, _, r), Some(bb)) = (&it.shape, board.outline) {
        let keep = rules.min.copper_edge_clearance;
        if p.x - r - bb.min.x < keep - EPS
          || bb.max.x - p.x - r < keep - EPS
          || p.y - r - bb.min.y < keep - EPS
          || bb.max.y - p.y - r < keep - EPS
        {
          rep.copper_edge += 1;
          issues.push(Issue {
            kind: "copper_edge",
            at: *p,
            detail: format!("via net {}", name(it.net)),
          });
        }
      }
    }
  }

  // Unconnected: per net, components holding a pad or the plane, minus one.
  let mut per_net: HashMap<u32, Vec<usize>> = HashMap::new();
  for (i, it) in items.iter().enumerate() {
    if it.kind == Kind::Pad && it.net != 0 && !it.layers.is_empty() {
      let root = uf.find(i);
      per_net.entry(it.net).or_default().push(root);
    }
  }
  for (k, (net, _, _)) in fills.iter().enumerate() {
    let root = uf.find(fill_base + k);
    per_net.entry(*net).or_default().push(root);
  }
  for &k in &assumed_planes {
    let Some((_, net)) = planes.get(k) else { continue };
    if per_net.contains_key(net) {
      let root = uf.find(items.len() + k);
      per_net.entry(*net).or_default().push(root);
    }
  }
  let mut nets: Vec<_> = per_net.into_iter().collect();
  nets.sort_by_key(|(n, _)| *n);
  for (net, mut roots) in nets {
    roots.sort_unstable();
    roots.dedup();
    if roots.len() > 1 {
      rep.unconnected += roots.len() - 1;
      // Locate the first pad of each extra component.
      let mut shown = 0;
      for &r in roots.iter().skip(1) {
        let found = items
          .iter()
          .enumerate()
          .find(|(i, it)| it.kind == Kind::Pad && it.net == net && uf.find(*i) == r);
        let at = found
          .and_then(|(_, it)| {
            it.hole.map(|h| h.0).or_else(|| {
              it.shape
                .bbox()
                .map(|b| Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0))
            })
          })
          .unwrap_or_default();
        // The nearest end of any track of this net: how far it stops short.
        let stop = found.map(|(_, pad)| {
          items
            .iter()
            .filter(|o| {
              o.kind == Kind::Track && o.net == net && o.layers.iter().any(|l| pad.layers.contains(l))
            })
            .filter_map(|o| o.ends)
            .flat_map(|(p, q)| [p, q])
            .map(|e| pad.shape.sdf(e))
            .fold(f64::INFINITY, f64::min)
        });
        if shown < 6 {
          issues.push(Issue {
            kind: "unconnected",
            at,
            detail: format!(
              "net {} split into {} parts; nearest same-net track end is {:.4} mm outside this pad",
              name(net),
              roots.len(),
              stop.unwrap_or(f64::NAN)
            ),
          });
          shown += 1;
        }
      }
    }
  }
  rep.issues = issues;
  rep
}

#[cfg(test)]
mod tests {
  use super::*;
  use autolayout_sexpr::parse;

  /// 20 x 10 board. Net A: pad 1 (a 3.5 x 1.6 roundrect, ratio 0.25, the C22.1
  /// shape) at (5,5) and pad 2 (0.8 square) at (15,5). Net B: pad 3 at (10,8).
  fn board(copper: &str) -> String {
    format!(
      r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd roundrect (at 5 5) (size 3.5 1.6) (layers "F.Cu") (roundrect_rratio 0.25) (net 1 "A"))
    (pad "2" smd rect (at 15 5) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "3" smd rect (at 10 8) (size 0.8 0.8) (layers "F.Cu") (net 2 "B")))
  {copper}
)"#
    )
  }

  fn run(copper: &str) -> VerifyReport {
    let doc = parse(&board(copper)).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
    r.min.copper_edge_clearance = 0.3;
    verify(&doc, &b, &r, &[])
  }

  fn seg(a: (f64, f64), b: (f64, f64), net: u32) -> String {
    format!(
      "(segment (start {} {}) (end {} {}) (width 0.1) (layer \"F.Cu\") (net {net}))",
      a.0, a.1, b.0, b.1
    )
  }

  #[test]
  fn a_track_joining_two_pads_is_clean() {
    let r = run(&seg((5.0, 5.0), (15.0, 5.0), 1));
    assert!(r.clean(), "{r:?}");
  }

  #[test]
  fn no_copper_leaves_the_net_unconnected() {
    let r = run("");
    assert_eq!(r.unconnected, 1);
  }

  /// The C22.1 failure: pad 1 spans x 3.25..6.75, y 4.2..5.8, corner radius
  /// 0.4. A track that ends 0.03 in from the right edge and 0.02 down from the
  /// top (6.72, 4.22) sits in the rounded-off corner: no copper there. KiCad
  /// reported exactly this as a dangling track and an unconnected pad.
  #[test]
  fn a_track_ending_in_a_rounded_corner_is_dangling() {
    let r = run(&seg((6.72, 4.22), (15.0, 4.22), 1));
    assert_eq!(r.dangling_tracks, 1, "{r:?}");
    assert!(r.unconnected >= 1, "{r:?}");
    // Ending well inside the same pad is fine.
    let ok = run(&seg((6.0, 5.0), (15.0, 5.0), 1));
    assert_eq!((ok.dangling_tracks, ok.unconnected), (0, 0), "{ok:?}");
  }

  #[test]
  fn two_vias_one_pitch_apart_break_hole_to_hole() {
    // Holes 0.2 mm, rule 0.1 (explicit rules): centres 0.25 apart leave 0.05.
    let vias = "(via (at 10 2) (size 0.4) (drill 0.2) (layers \"F.Cu\" \"B.Cu\") (net 1)) (via (at 10.25 2) (size 0.4) (drill 0.2) (layers \"F.Cu\" \"B.Cu\") (net 1))";
    let r = run(&format!("{vias} {}", seg((5.0, 5.0), (15.0, 5.0), 1)));
    assert_eq!(r.hole_to_hole, 1, "{r:?}");
  }

  #[test]
  fn clearance_between_nets_is_measured_on_the_true_shapes() {
    // A net-A track 0.05 from net B's pad 3 (pad edge y = 7.6, track at 7.5
    // with half-width 0.05 reaches 7.55): gap 0.05 < 0.1.
    let r = run(&format!(
      "{} {}",
      seg((5.0, 5.0), (15.0, 5.0), 1),
      seg((8.0, 7.5), (12.0, 7.5), 1)
    ));
    assert!(r.clearance >= 1, "{r:?}");
  }

  #[test]
  fn garbage_copper_does_not_panic() {
    let r = run("(segment) (via) (segment (start 1) (end)) (via (at x y))");
    assert_eq!(r.segments + r.vias, 0);
  }

  /// KiCad counts overlapping copper of different nets as `shorting_items`
  /// and a too-small positive gap as `clearance` (Freerouting's m1: 2 + 2,
  /// the verifier used to call all four clearance).
  #[test]
  fn overlap_is_a_short_and_a_small_gap_is_a_clearance() {
    let short = run(&format!(
      "{} {}",
      seg((5.0, 5.0), (15.0, 5.0), 1),
      seg((8.0, 7.7), (12.0, 7.7), 1)
    ));
    assert!(short.shorting >= 1 && short.clearance == 0, "{short:?}");
    let near = run(&format!(
      "{} {}",
      seg((5.0, 5.0), (15.0, 5.0), 1),
      seg((8.0, 7.5), (12.0, 7.5), 1)
    ));
    assert!(near.clearance >= 1 && near.shorting == 0, "{near:?}");
  }

  /// A plane is assumed solid; the report must say so rather than let
  /// "unconnected 0" read as a check of the fill.
  #[test]
  fn assuming_a_plane_is_reported_as_a_limit() {
    let doc = parse(&board("")).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let rep = verify(&doc, &b, &r, &[("F.Cu".to_string(), 1)]);
    assert_eq!(rep.plane_items_assumed, 2, "both net-A pads sit on F.Cu");
    assert!(rep.summary().contains("LIMIT"));
    assert!(!run("").summary().contains("LIMIT"));
  }

  fn fill(net: u32, layer: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> String {
    format!(
      "(zone (net {net}) (net_name \"n\") (layer \"{layer}\") (filled_polygon (layer \"{layer}\") (pts (xy {x0} {y0}) (xy {x1} {y0}) (xy {x1} {y1}) (xy {x0} {y1}))))"
    )
  }

  fn run_planes(copper: &str, planes: &[(String, u32)]) -> VerifyReport {
    let doc = parse(&board(copper)).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
    r.min.copper_edge_clearance = 0.3;
    verify(&doc, &b, &r, planes)
  }

  /// KiCad's findings on Freerouting's m1 were separate fill polygons of one
  /// net with nothing joining them. One fill under both pads joins them.
  #[test]
  fn one_fill_under_both_pads_joins_them() {
    let r = run(&fill(1, "F.Cu", 2.0, 3.0, 17.0, 7.0));
    assert_eq!(r.unconnected, 0, "{r:?}");
    assert_eq!(r.fill_polygons, 1);
  }

  #[test]
  fn two_separate_fills_leave_the_net_split() {
    let z = format!(
      "{} {}",
      fill(1, "F.Cu", 2.0, 3.0, 8.0, 7.0),
      fill(1, "F.Cu", 12.0, 3.0, 18.0, 7.0)
    );
    let r = run(&z);
    assert_eq!(r.unconnected, 1, "{r:?}");
  }

  #[test]
  fn a_fill_island_touching_nothing_is_unconnected() {
    let z = format!(
      "{} {}",
      fill(1, "F.Cu", 2.0, 3.0, 17.0, 7.0),
      fill(1, "F.Cu", 9.0, 1.0, 10.0, 2.0)
    );
    let r = run(&z);
    assert_eq!(r.unconnected, 1, "{r:?}");
    // A track that ties the island to the main fill repairs it.
    let both = format!("{z} (segment (start 9.5 1.5) (end 9.5 4.0) (width 0.1) (layer \"F.Cu\") (net 1))");
    assert_eq!(run(&both).unconnected, 0);
  }

  #[test]
  fn a_fill_of_another_net_or_layer_joins_nothing() {
    // Net A's pads stay apart (1), and the stray fill is itself a component
    // of its own net (B's pad + the fill island: 1 more), or of net A on the
    // wrong layer (3 components: 2 more).
    let other_net = run(&fill(2, "F.Cu", 2.0, 3.0, 17.0, 7.0));
    assert_eq!(other_net.unconnected, 2, "{other_net:?}");
    let other_layer = run(&fill(1, "B.Cu", 2.0, 3.0, 17.0, 7.0));
    assert_eq!(other_layer.unconnected, 2, "{other_layer:?}");
  }

  #[test]
  fn a_declared_plane_with_a_fill_is_read_not_assumed() {
    let with_fill = run_planes(&fill(1, "F.Cu", 2.0, 3.0, 8.0, 7.0), &[("F.Cu".to_string(), 1)]);
    assert_eq!(with_fill.plane_items_assumed, 0);
    assert_eq!(
      with_fill.unconnected, 1,
      "pad 2 is outside the fill: {with_fill:?}"
    );
    let without = run_planes("", &[("F.Cu".to_string(), 1)]);
    assert_eq!(without.plane_items_assumed, 2);
    assert_eq!(without.unconnected, 0);
  }
}
