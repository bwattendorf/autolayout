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

use autolayout_board::{BBox, Board, Point, arc_points};
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

/// A probe point matches copper within this distance (KiCad prints 4 decimals).
const PROBE_REACH: f64 = 0.005;

/// How far past a pad's outer radius a same-net fill may stop and still count as
/// touching it (spokes of a thermal relief end at the pad edge).
const RELIEF_REACH: f64 = 0.05;

/// Distance from `p` to the nearest edge of `poly`; 0 if `p` is inside it.
fn polygon_distance(poly: &[Point], p: Point) -> f64 {
  if crate::shape::polygon_contains(poly, p) {
    return 0.0;
  }
  let mut d = f64::INFINITY;
  for i in 0..poly.len() {
    if let (Some(&a), Some(&b)) = (poly.get(i), poly.get((i + 1) % poly.len())) {
      d = d.min(crate::shape::seg_dist(p, a, b));
    }
  }
  d
}

/// Largest half-extent of a pad from its centre (its radius if round).
fn outer_radius(s: &Shape) -> f64 {
  match s {
    Shape::RoundRect(_, h, _, _) => h.x.max(h.y),
    Shape::Capsule(a, b, r) => r + (*b - *a).length() / 2.0,
    Shape::Union(_) | Shape::Polygon(_) => s.bbox().map_or(0.0, |b| b.width().max(b.height()) / 2.0),
  }
}

/// Shortfalls below this are not counted (float noise). KiCad flags 0.0006 mm
/// (round 9), so this must stay far below a micrometre.
const CLEARANCE_SLACK: f64 = 0.0002;

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
  /// One block of lines per probe point (see [`verify_with_probes`]).
  pub probe_notes: Vec<String>,
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
  /// `REF.PIN` for pads, empty otherwise: names a component's members.
  label: String,
  /// Local clearance override of a pad (its own or its footprint's).
  clearance: Option<f64>,
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
  verify_with_probes(doc, board, rules, planes, &[])
}

/// Like [`verify`], and for each probe point also say which copper lies on it
/// and which part of its net that copper belongs to (`probe_notes`). This is
/// how a point from a KiCad report is matched to a verifier part exactly,
/// where the printed track-end boxes of two parts can overlap.
pub fn verify_with_probes(
  doc: &Document,
  board: &Board,
  rules: &DesignRules,
  planes: &[(String, u32)],
  probes: &[Point],
) -> VerifyReport {
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
            label: String::new(),
            clearance: None,
            size: 0.0,
          });
        }
        continue;
      }
      // Pads may share a number inside one footprint (SW2 has two pads "2");
      // later duplicates get a suffix so a label names one pad.
      let dup = fp
        .pads
        .iter()
        .take_while(|q| !std::ptr::eq(*q, pad))
        .filter(|q| q.number == pad.number)
        .count();
      let label = if dup == 0 {
        format!("{}.{}", fp.reference, pad.number)
      } else {
        format!("{}.{}#{}", fp.reference, pad.number, dup + 1)
      };
      items.push(Item {
        kind: Kind::Pad,
        net: pad.net.unwrap_or(0),
        shape: pad_copper(pad),
        layers: resolve_layers(&pad.layers, &cu),
        hole: pad.drill.map(|d| (pad.at, d.x.max(d.y))),
        ends: None,
        anchor: Some(pad.at),
        label,
        clearance: pad.clearance,
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
              label: String::new(),
              clearance: None,
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
          label: String::new(),
          clearance: None,
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
        // An item's local override replaces its class clearance; a pair needs the
        // larger of the two, and never less than the board minimum.
        let eff = |it: &Item| {
          it.clearance
            .unwrap_or_else(|| rules.class_for_net(name(it.net)).clearance)
        };
        let need = eff(a).max(eff(b)).max(rules.min.clearance);
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
        // A custom pad: its centre stands for it.
        Shape::Union(_) => it.anchor.is_some_and(hit),
      };
      // A through-hole pad with thermal relief is reached by spokes across a gap
      // (g2i's connector pins, round 9: the verifier had them 9 parts apart and
      // KiCad one): the pour then stops short of the pad's own copper, so no
      // point inside the pad lies in the fill. A same-net fill that comes within
      // a hair of the pad's outer radius touches it.
      let near_pad = it.kind == Kind::Pad
        && it
          .anchor
          .is_some_and(|c| polygon_distance(poly, c) <= outer_radius(&it.shape) + RELIEF_REACH);
      if touches || near_pad {
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
  let mut part_of: HashMap<usize, String> = HashMap::new();
  let mut nets: Vec<_> = per_net.into_iter().collect();
  nets.sort_by_key(|(n, _)| *n);
  for (net, mut roots) in nets {
    roots.sort_unstable();
    roots.dedup();
    if roots.len() < 2 {
      for &r in &roots {
        part_of.insert(r, format!("net {}: the only part", name(net)));
      }
      continue;
    }
    rep.unconnected += roots.len() - 1;
    // Describe every component by what is in it, so it can be compared with
    // KiCad's report member by member. (KiCad prints one point per zone, not
    // per island: all 13 GND fragments of Freerouting's m1 sit at the zone's
    // anchor (50.425, 50.425), so a fill island has no location to match.)
    struct Comp {
      root: usize,
      pads: Vec<String>,
      tracks: usize,
      /// Box around every track end of this component (to match KiCad's track coordinates).
      tbox: Option<BBox>,
      vias: usize,
      fills: Vec<(String, BBox)>,
      at: Point,
    }
    let mut comps: Vec<Comp> = roots
      .iter()
      .map(|&r| Comp {
        root: r,
        pads: Vec::new(),
        tracks: 0,
        tbox: None,
        vias: 0,
        fills: Vec::new(),
        at: Point::default(),
      })
      .collect();
    for (i, it) in items.iter().enumerate() {
      if it.net != net {
        continue;
      }
      let r = uf.find(i);
      if let Some(c) = comps.iter_mut().find(|c| c.root == r) {
        match it.kind {
          Kind::Pad if !it.layers.is_empty() => {
            c.pads.push(it.label.clone());
            if c.pads.len() == 1 {
              c.at = it.anchor.unwrap_or_default();
            }
          }
          Kind::Track => {
            c.tracks += 1;
            if let Some((p, q)) = it.ends {
              c.tbox = BBox::union(c.tbox, BBox::from_points([p, q]));
            }
          }
          Kind::Via => c.vias += 1,
          Kind::Pad => {}
        }
      }
    }
    for (k, (n, li, poly)) in fills.iter().enumerate() {
      if *n != net {
        continue;
      }
      let r = uf.find(fill_base + k);
      if let (Some(c), Some(bb)) = (
        comps.iter_mut().find(|c| c.root == r),
        BBox::from_points(poly.iter().copied()),
      ) {
        if c.pads.is_empty() && c.fills.is_empty() {
          c.at = bb.min;
        }
        c.fills.push((cu.get(*li).cloned().unwrap_or_default(), bb));
      }
    }
    // The main component is the one with the most pads (then fills); the rest are reported.
    comps.sort_by(|a, b| {
      (b.pads.len(), b.fills.len(), b.tracks)
        .cmp(&(a.pads.len(), a.fills.len(), a.tracks))
        .then(a.root.cmp(&b.root))
    });
    for (k, c) in comps.iter().enumerate() {
      part_of.insert(
        c.root,
        format!(
          "net {} part {} of {}{}",
          name(net),
          k + 1,
          comps.len(),
          if k == 0 { " (the main part)" } else { "" }
        ),
      );
    }
    for (k, c) in comps.iter().enumerate().skip(1) {
      if k > 40 {
        issues.push(Issue {
          kind: "unconnected",
          at: c.at,
          detail: format!(
            "net {}: {} more components not listed",
            name(net),
            comps.len() - k
          ),
        });
        break;
      }
      let fills = c
        .fills
        .iter()
        .map(|(l, b)| {
          format!(
            "{l} fill ({:.3},{:.3})-({:.3},{:.3})",
            b.min.x, b.min.y, b.max.x, b.max.y
          )
        })
        .collect::<Vec<_>>()
        .join("; ");
      issues.push(Issue {
        kind: "unconnected",
        at: c.at,
        detail: format!(
          "net {} part {} of {} (main part has {} pads): pads [{}], {} tracks{}, {} vias{}",
          name(net),
          k + 1,
          comps.len(),
          comps.first().map_or(0, |m| m.pads.len()),
          c.pads.join(" "),
          c.tracks,
          c.tbox
            .map(|b| format!(
              " with ends in ({:.3},{:.3})-({:.3},{:.3})",
              b.min.x, b.min.y, b.max.x, b.max.y
            ))
            .unwrap_or_default(),
          c.vias,
          if fills.is_empty() {
            String::new()
          } else {
            format!(", fills: {fills}")
          }
        ),
      });
    }
  }
  for probe in probes {
    let mut lines = vec![format!("probe ({:.4}, {:.4}):", probe.x, probe.y)];
    for (i, it) in items.iter().enumerate() {
      if it.shape.sdf(*probe) <= PROBE_REACH {
        let what = match it.kind {
          Kind::Pad if it.layers.is_empty() => continue,
          Kind::Pad => format!("pad {}", it.label),
          Kind::Track => "track".to_string(),
          Kind::Via => "via".to_string(),
        };
        let layers: Vec<&str> = it
          .layers
          .iter()
          .filter_map(|l| cu.get(*l).map(String::as_str))
          .collect();
        let part = part_of
          .get(&uf.find(i))
          .cloned()
          .unwrap_or_else(|| "a component with no pad or fill".to_string());
        lines.push(format!(
          "  {what} net {} on {} -> {part}",
          name(it.net),
          if layers.len() > 3 {
            "all layers".to_string()
          } else {
            layers.join("+")
          }
        ));
      }
    }
    for (k, (net, li, poly)) in fills.iter().enumerate() {
      if crate::shape::polygon_contains(poly, *probe) {
        let part = part_of.get(&uf.find(fill_base + k)).cloned().unwrap_or_default();
        lines.push(format!(
          "  fill {} net {} -> {part}",
          cu.get(*li).map_or("?", String::as_str),
          name(*net)
        ));
      }
    }
    if lines.len() == 1 {
      lines.push(format!("  no copper within {PROBE_REACH} mm"));
    }
    rep.probe_notes.push(lines.join("\n"));
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

  /// Each extra component is described by its members, so it can be compared
  /// with KiCad's report item by item instead of by one arbitrary location.
  #[test]
  fn unconnected_components_are_listed_with_their_members() {
    let z = format!(
      "{} {}",
      fill(1, "F.Cu", 2.0, 3.0, 8.0, 7.0),
      fill(1, "F.Cu", 12.0, 3.0, 18.0, 7.0)
    );
    let r = run(&z);
    let u: Vec<&Issue> = r.issues.iter().filter(|i| i.kind == "unconnected").collect();
    assert_eq!(u.len(), 1, "{u:?}");
    assert!(u[0].detail.contains("pads [U1."), "{}", u[0].detail);
    assert!(u[0].detail.contains("F.Cu fill ("), "{}", u[0].detail);
    // A lone island has no pads and says where it is.
    let island = format!(
      "{} {}",
      fill(1, "F.Cu", 2.0, 3.0, 17.0, 7.0),
      fill(1, "F.Cu", 9.0, 1.0, 10.0, 2.0)
    );
    let r = run(&island);
    let d = &r.issues.iter().find(|i| i.kind == "unconnected").unwrap().detail;
    assert!(
      d.contains("pads []") && d.contains("(9.000,1.000)-(10.000,2.000)"),
      "{d}"
    );
  }

  fn run_probes(copper: &str, probes: &[Point]) -> VerifyReport {
    let doc = parse(&board(copper)).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
    r.min.copper_edge_clearance = 0.3;
    verify_with_probes(&doc, &b, &r, &[], probes)
  }

  /// KiCad names points, not parts; a probe says exactly which part of its net
  /// the copper at a point belongs to (track-end boxes of two parts overlap).
  #[test]
  fn a_probe_names_the_copper_and_its_part() {
    let z = format!(
      "{} {}",
      fill(1, "F.Cu", 2.0, 3.0, 8.0, 7.0),
      fill(1, "F.Cu", 12.0, 3.0, 18.0, 7.0)
    );
    let r = run_probes(
      &z,
      &[Point::new(5.0, 5.0), Point::new(15.0, 5.0), Point::new(0.5, 9.5)],
    );
    assert_eq!(r.probe_notes.len(), 3);
    assert!(
      r.probe_notes[0].contains("pad U1.1") && r.probe_notes[0].contains("part"),
      "{}",
      r.probe_notes[0]
    );
    assert!(r.probe_notes[0].contains("fill F.Cu"), "{}", r.probe_notes[0]);
    assert!(r.probe_notes[1].contains("pad U1.2"), "{}", r.probe_notes[1]);
    // The two pads are in different parts, so the part text differs.
    let part = |n: &str| {
      n.lines()
        .find(|l| l.contains("pad U1."))
        .unwrap()
        .split("-> ")
        .nth(1)
        .unwrap()
        .to_string()
    };
    assert_ne!(part(&r.probe_notes[0]), part(&r.probe_notes[1]));
    assert!(
      r.probe_notes[2].contains("no copper within"),
      "{}",
      r.probe_notes[2]
    );
  }

  #[test]
  fn a_probe_on_a_track_end_finds_the_track() {
    let r = run_probes(&seg((5.0, 5.0), (15.0, 5.0), 1), &[Point::new(10.0, 5.0)]);
    assert!(r.probe_notes[0].contains("track net A"), "{}", r.probe_notes[0]);
    assert!(r.probe_notes[0].contains("the only part"), "{}", r.probe_notes[0]);
  }

  /// SW2 on m1 has two pads numbered 2; a label must name one pad.
  #[test]
  fn duplicate_pad_numbers_get_distinct_labels() {
    let src = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "SW2")
    (pad "2" smd rect (at 5 5) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 15 5) (size 1 1) (layers "F.Cu") (net 1 "A"))))"#;
    let doc = parse(src).unwrap();
    let b = Board::read(&doc).unwrap();
    let r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
    let rep = verify_with_probes(&doc, &b, &r, &[], &[Point::new(5.0, 5.0), Point::new(15.0, 5.0)]);
    assert!(
      rep.probe_notes[0].contains("pad SW2.2 ") && rep.probe_notes[1].contains("pad SW2.2#2"),
      "{:?}",
      rep.probe_notes
    );
  }

  /// Regression (round 7, KiCad 9.0.1): J10 on m1 carries a footprint-level
  /// `(clearance 0.13)`; our router and verifier ignored it and KiCad reported
  /// 8 violations at gaps of 0.107-0.124 mm that the verifier called clean.
  #[test]
  fn a_footprint_clearance_override_is_honoured() {
    // Net A's track runs 0.3 mm from net B's pad 3 (pad edge y = 7.6, track
    // edge 7.3): fine under the 0.1 rule, a violation under a 0.5 override.
    let copper = format!(
      "{} {}",
      seg((5.0, 5.0), (15.0, 5.0), 1),
      seg((8.0, 7.25), (12.0, 7.25), 1)
    );
    let judge = |text: String| {
      let doc = parse(&text).unwrap();
      let b = Board::read(&doc).unwrap();
      let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
      r.min.copper_edge_clearance = 0.3;
      verify(&doc, &b, &r, &[])
    };
    let plain = judge(board(&copper));
    assert_eq!(plain.clearance + plain.shorting, 0, "{plain:?}");
    let strict = judge(board(&copper).replace(
      "(property \"Reference\" \"U1\")",
      "(property \"Reference\" \"U1\") (clearance 0.5)",
    ));
    assert!(strict.clearance >= 1, "{strict:?}");
    // An override that is smaller than the rule does not relax it below the
    // board minimum.
    let loose = judge(board(&copper).replace(
      "(property \"Reference\" \"U1\")",
      "(property \"Reference\" \"U1\") (clearance 0.01)",
    ));
    assert_eq!(loose.clearance + loose.shorting, 0, "{loose:?}");
  }

  /// Regression (round 9, board r): U6 pads 15/16 are `custom` pads, a 0.115 mm
  /// anchor plus a polygon 0.845 mm long. Modelled as the anchor alone, a track
  /// beside the polygon looked clear and KiCad reported a short and a 0.015 mm
  /// clearance violation.
  #[test]
  fn a_custom_pad_is_its_primitives_not_just_its_anchor() {
    let custom = |prims: &str| {
      format!(
        r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B")
  (gr_line (start 0 0) (end 20 0) (layer "Edge.Cuts"))
  (gr_line (start 20 0) (end 20 10) (layer "Edge.Cuts"))
  (gr_line (start 20 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U6")
    (pad "15" smd custom (at 10 8 0) (size 0.1 0.1) (layers "F.Cu") (net 2 "B")
      (options (clearance outline) (anchor circle)) {prims}))
  (segment (start 10.3 8.2) (end 10.6 8.2) (width 0.1) (layer "F.Cu") (net 1)))"#
      )
    };
    let judge = |text: String| {
      let doc = parse(&text).unwrap();
      let b = Board::read(&doc).unwrap();
      let mut r = DesignRules::explicit(0.1, 0.1, 0.4, 0.2).unwrap();
      r.min.copper_edge_clearance = 0.3;
      verify(&doc, &b, &r, &[])
    };
    let anchor_only = judge(custom(""));
    assert_eq!(anchor_only.clearance + anchor_only.shorting, 0, "{anchor_only:?}");
    let poly = "(primitives (gr_poly (pts (xy -0.6 -0.06) (xy 0.6 -0.06) (xy 0.6 0.06) (xy -0.6 0.06)) (width 0.08) (fill yes)))";
    let full = judge(custom(poly));
    assert!(full.clearance >= 1, "{full:?}");
  }

  /// Regression (round 9, g2i): a through-hole pad with thermal relief is
  /// joined to its pour by spokes, so the fill stops a gap short of the pad.
  /// Net A's pad 1 (3.5 x 1.6 roundrect at (5,5)) is the pad here; a fill whose
  /// edge is 0.02 beyond the pad's right edge touches it, one 0.5 beyond does not.
  #[test]
  fn a_fill_that_stops_at_the_pad_edge_still_touches_it() {
    let near = "(zone (net 1) (net_name \"n\") (layer \"F.Cu\") (filled_polygon (layer \"F.Cu\") (pts (xy 6.77 3) (xy 17 3) (xy 17 7) (xy 6.77 7))))".to_string();
    let far = near.replace("6.77", "7.25");
    let r_near = run(&near);
    let r_far = run(&far);
    // Pad 2 at (15,5) lies inside both fills; pad 1 joins only through the near one.
    assert_eq!(r_near.unconnected, 0, "{r_near:?}");
    assert_eq!(r_far.unconnected, 1, "{r_far:?}");
  }
}
