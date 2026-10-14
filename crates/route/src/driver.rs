//! Greedy sequential routing over a whole board (M7.2 baseline): plane taps
//! first, then each signal net shortest-first, every net blocking the ones
//! after it. No rip-up: a net that cannot be finished stays unfinished. This
//! is the number negotiated congestion (M7.3) has to beat.

use autolayout_board::Board;

use crate::model::RoutingModel;
use crate::search::{Costs, NetRoute, Router};

#[derive(Debug, Clone, Default)]
pub struct Summary {
  pub routes: Vec<NetRoute>,
  pub nets_total: usize,
  pub nets_complete: usize,
  pub pads_total: usize,
  pub pads_connected: usize,
  pub length_mm: f64,
  pub vias: usize,
}

/// (half-perimeter, net code, pads): a net queued for routing.
pub type Ordered = (f64, u32, Vec<(usize, usize)>);

/// Half-perimeter of a net's pad box, used to order nets short-first.
fn hpwl(board: &Board, pads: &[(usize, usize)]) -> f64 {
  let pts = pads
    .iter()
    .filter_map(|&(f, p)| board.footprints.get(f)?.pads.get(p).map(|p| p.at));
  autolayout_board::BBox::from_points(pts).map_or(0.0, |b| b.width() + b.height())
}

/// Plane nets: one via per pad that does not already reach the plane. Adds the
/// plane routes to `s` (and permanently claims their vias in `router`).
pub fn tap_planes(
  board: &Board,
  model: &RoutingModel,
  router: &mut Router,
  by_net: &std::collections::HashMap<u32, Vec<(usize, usize)>>,
  s: &mut Summary,
) {
  let plane_nets: Vec<u32> = model.planes.iter().map(|p| p.net).collect();
  let mut codes: Vec<u32> = by_net
    .keys()
    .copied()
    .filter(|n| plane_nets.contains(n))
    .collect();
  codes.sort_unstable();
  for net in codes {
    let mut agg = NetRoute {
      net,
      ..NetRoute::default()
    };
    let mut pads = by_net.get(&net).cloned().unwrap_or_default();
    pads.sort_unstable();
    for &(f, p) in &pads {
      if let Some(r) = router.tap_plane(board, f, p, net) {
        agg.vias.extend(r.vias);
        agg.stubs.extend(r.stubs);
        agg.connected.extend(r.connected);
        agg.failed.extend(r.failed);
      }
    }
    s.nets_total += 1;
    s.pads_total += pads.len();
    s.pads_connected += agg.connected.len();
    s.vias += agg.vias.len();
    if agg.failed.is_empty() {
      s.nets_complete += 1;
    }
    s.routes.push(agg);
  }
}

/// Signal nets with two or more pads, shortest half-perimeter first (ties by net code).
pub fn order_signal_nets(
  board: &Board,
  model: &RoutingModel,
  by_net: &std::collections::HashMap<u32, Vec<(usize, usize)>>,
) -> Vec<Ordered> {
  let plane_nets: Vec<u32> = model.planes.iter().map(|p| p.net).collect();
  let mut codes: Vec<u32> = by_net
    .keys()
    .copied()
    .filter(|&n| n != 0 && !plane_nets.contains(&n))
    .collect();
  codes.sort_unstable();
  let mut signal: Vec<Ordered> = codes
    .iter()
    .filter_map(|&n| {
      let mut pads = by_net.get(&n)?.clone();
      pads.sort_unstable();
      (pads.len() >= 2).then(|| (hpwl(board, &pads), n, pads))
    })
    .collect();
  signal.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
  signal
}

pub fn route_all(board: &Board, model: &RoutingModel, costs: Costs) -> Summary {
  let mut router = Router::new(model, costs);
  let by_net = board.pads_by_net();
  let mut s = Summary::default();
  tap_planes(board, model, &mut router, &by_net, &mut s);
  for (_, net, pads) in order_signal_nets(board, model, &by_net) {
    let r = router.route_net(board, net, &pads);
    s.nets_total += 1;
    s.pads_total += pads.len();
    s.pads_connected += r.connected.len();
    if r.failed.is_empty() {
      s.nets_complete += 1;
    }
    s.length_mm += r.length_mm(model.pitch);
    s.vias += r.vias.len();
    s.routes.push(r);
  }
  s
}

/// Test helper: write `routes` into the board text `src` and verify the result
/// from the file alone, with the rules the model was built for.
#[cfg(test)]
pub(crate) fn judge(
  src: &str,
  board: &Board,
  model: &RoutingModel,
  routes: &[NetRoute],
) -> crate::verify::VerifyReport {
  use crate::rules::DesignRules;
  let p = &model.profile;
  let mut rules = DesignRules::explicit(p.track_width, p.clearance, p.via_diameter, p.via_drill).unwrap();
  rules.min.copper_edge_clearance = 0.3;
  let mut doc = autolayout_sexpr::parse(src).unwrap();
  let summary = Summary {
    routes: routes.to_vec(),
    ..Summary::default()
  };
  crate::write::add_routes(&mut doc, model, board, &summary).unwrap();
  let text = doc.to_string();
  let doc = autolayout_sexpr::parse(&text).unwrap();
  let b = Board::read(&doc).unwrap();
  let planes: Vec<(String, u32)> = model.planes.iter().map(|p| (p.layer.clone(), p.net)).collect();
  crate::verify::verify(&doc, &b, &rules, &planes)
}

#[cfg(test)]
mod tests {
  use super::judge;
  use super::*;
  use crate::rules::DesignRules;
  use autolayout_sexpr::parse;

  /// 30 x 12 mm, two signal layers and a plane layer In1 = G. Net A has pads
  /// at x = 5, 15, 25 (y = 6); a column of B pads at x = 15 (y = 3, 9) and a
  /// thru-hole wall pad of net C at (15, 6)... except the middle A pad sits
  /// on the wall, so A must go around on F.Cu or over by via.
  fn board(extra: &str) -> String {
    format!(
      r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (4 "In1.Cu" power) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "G")
  (gr_line (start 0 0) (end 30 0) (layer "Edge.Cuts"))
  (gr_line (start 30 0) (end 30 12) (layer "Edge.Cuts"))
  (gr_line (start 30 12) (end 0 12) (layer "Edge.Cuts"))
  (gr_line (start 0 12) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 5 6) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 15 6) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "3" smd rect (at 25 6) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "4" smd rect (at 15 3) (size 1 1) (layers "F.Cu") (net 2 "B"))
    (pad "5" smd rect (at 15 9) (size 1 1) (layers "F.Cu") (net 2 "B"))
    (pad "6" smd rect (at 8 3) (size 1 1) (layers "F.Cu") (net 3 "G"))
    {extra})
)"#
    )
  }

  fn setup(extra: &str) -> (Board, RoutingModel) {
    let doc = parse(&board(extra)).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.5;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &b, &r, &p, &[("In1.Cu".to_string(), "G".to_string())]).unwrap();
    (b, m)
  }

  #[test]
  fn routes_a_three_pad_net_and_the_self_check_agrees() {
    let (b, m) = setup("");
    let s = route_all(&b, &m, Costs::default());
    let a = s.routes.iter().find(|r| r.net == 1).unwrap();
    assert!(a.failed.is_empty(), "failed: {:?}", a.failed);
    assert_eq!(a.connected.len(), 3);
    assert!(!a.runs.is_empty());
    let c = judge(&board(""), &b, &m, &s.routes);
    assert!(c.clean(), "{c:?}");
  }

  #[test]
  fn same_board_same_routes() {
    let (b, m) = setup("");
    let x = route_all(&b, &m, Costs::default());
    let y = route_all(&b, &m, Costs::default());
    assert_eq!(x.routes, y.routes);
  }

  #[test]
  fn plane_net_pad_gets_a_via() {
    let (b, m) = setup("");
    let s = route_all(&b, &m, Costs::default());
    let g = s.routes.iter().find(|r| r.net == 3).unwrap();
    assert_eq!(g.vias.len(), 1);
    assert_eq!(g.connected.len(), 1);
  }

  #[test]
  fn an_enclosed_pad_fails_cleanly() {
    // Pad of net A inside a ring of net B pads on both layers (thru-hole ring
    // pads): no cell around it is usable, so it must be reported, not hang.
    let ring: String = [
      (-1.0, -1.0),
      (0.0, -1.0),
      (1.0, -1.0),
      (-1.0, 0.0),
      (1.0, 0.0),
      (-1.0, 1.0),
      (0.0, 1.0),
      (1.0, 1.0),
    ]
    .iter()
    .map(|(dx, dy)| {
      format!(
        r#"(pad "9" thru_hole circle (at {} {}) (size 1.2 1.2) (drill 0.5) (layers "*.Cu") (net 2 "B"))"#,
        25.0 + dx * 1.0,
        9.5 + dy * 1.0
      )
    })
    .collect();
    let (b, m) = setup(&format!(
      r#"(pad "7" smd rect (at 25 9.5) (size 0.6 0.6) (layers "F.Cu") (net 1 "A")) {ring}"#
    ));
    let s = route_all(&b, &m, Costs::default());
    let a = s.routes.iter().find(|r| r.net == 1).unwrap();
    assert!(!a.failed.is_empty(), "the enclosed pad cannot be reached");
    assert!(s.nets_complete < s.nets_total);
  }
}

#[cfg(test)]
mod negotiation_tests {
  use super::judge;
  use super::*;
  use crate::negotiate::{Params, route_negotiated};
  use crate::rules::DesignRules;
  use autolayout_sexpr::parse;

  /// Four nets cross a corridor on a 2-layer board. Whatever greedy manages,
  /// negotiation must end with no contested cells, a clean self-check, never
  /// complete fewer nets than greedy on this board, and the same bytes twice.
  const BOARD: &str = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C") (net 4 "D")
  (gr_line (start 0 0) (end 16 0) (layer "Edge.Cuts"))
  (gr_line (start 16 0) (end 16 8) (layer "Edge.Cuts"))
  (gr_line (start 16 8) (end 0 8) (layer "Edge.Cuts"))
  (gr_line (start 0 8) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 2 2) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 14 2) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "3" smd rect (at 2 3.5) (size 0.8 0.8) (layers "F.Cu") (net 2 "B"))
    (pad "4" smd rect (at 14 3.5) (size 0.8 0.8) (layers "F.Cu") (net 2 "B"))
    (pad "5" smd rect (at 2 5) (size 0.8 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "6" smd rect (at 14 5) (size 0.8 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "7" smd rect (at 2 6.5) (size 0.8 0.8) (layers "F.Cu") (net 4 "D"))
    (pad "8" smd rect (at 14 6.5) (size 0.8 0.8) (layers "F.Cu") (net 4 "D"))
    (pad "9" thru_hole circle (at 8 2.75) (size 1.4 1.4) (drill 0.7) (layers "*.Cu") (net 2 "B"))
    (pad "10" thru_hole circle (at 8 5.75) (size 1.4 1.4) (drill 0.7) (layers "*.Cu") (net 3 "C")))
)"#;

  /// A 0.2 x 0.8 mm pad row at 0.5 mm pitch under 0.2 mm rules has no grid
  /// cell inside its pads; the outer pads must still be escaped, with a
  /// clean self-check.
  const FINE: &str = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C") (net 4 "D")
  (gr_line (start 0 0) (end 14 0) (layer "Edge.Cuts"))
  (gr_line (start 14 0) (end 14 10) (layer "Edge.Cuts"))
  (gr_line (start 14 10) (end 0 10) (layer "Edge.Cuts"))
  (gr_line (start 0 10) (end 0 0) (layer "Edge.Cuts"))
  (footprint "Q" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 5.0 5) (size 0.2 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 5.5 5) (size 0.2 0.8) (layers "F.Cu") (net 2 "B"))
    (pad "3" smd rect (at 6.0 5) (size 0.2 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "4" smd rect (at 6.5 5) (size 0.2 0.8) (layers "F.Cu") (net 4 "D"))
    (pad "5" smd rect (at 1 1) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "6" smd rect (at 1 3) (size 0.8 0.8) (layers "F.Cu") (net 2 "B"))
    (pad "7" smd rect (at 13 1) (size 0.8 0.8) (layers "F.Cu") (net 3 "C"))
    (pad "8" smd rect (at 13 3) (size 0.8 0.8) (layers "F.Cu") (net 4 "D")))
)"#;

  #[test]
  fn fine_pitch_pads_without_interior_cells_are_escaped() {
    let doc = parse(FINE).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &b, &r, &p, &[]).unwrap();
    let (n, _) = route_negotiated(&b, &m, Costs::default(), Params::default());
    // Stubs of adjacent 0.5 mm-pitch pads keep a legal distance, so they may
    // share corridor cells (`Costs::share_stubs`): all four pads escape.
    assert_eq!(
      n.nets_complete,
      4,
      "{:?}",
      n.routes.iter().map(|r| &r.failed).collect::<Vec<_>>()
    );
    let c = judge(FINE, &b, &m, &n.routes);
    assert_eq!(c.unconnected, 4 - n.nets_complete, "{c:?}");
    assert_eq!(
      c.dangling_tracks + c.hole_to_hole + c.clearance + c.shorting,
      0,
      "{c:?}"
    );
  }

  fn setup() -> (Board, RoutingModel) {
    let doc = parse(BOARD).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &b, &r, &p, &[]).unwrap();
    (b, m)
  }

  #[test]
  fn negotiation_is_conflict_free_clean_and_no_worse_than_greedy() {
    let (b, m) = setup();
    let g = route_all(&b, &m, Costs::default());
    let (n, st) = route_negotiated(&b, &m, Costs::default(), Params::default());
    assert_eq!(st.conflicts.last(), Some(&0), "{:?}", st.conflicts);
    assert!(
      n.nets_complete >= g.nets_complete,
      "{} < {}",
      n.nets_complete,
      g.nets_complete
    );
    let c = judge(BOARD, &b, &m, &n.routes);
    assert!(c.clean(), "{c:?}");
  }

  #[test]
  fn negotiation_is_deterministic() {
    let (b, m) = setup();
    let (x, sx) = route_negotiated(&b, &m, Costs::default(), Params::default());
    let (y, sy) = route_negotiated(&b, &m, Costs::default(), Params::default());
    assert_eq!(x.routes, y.routes);
    assert_eq!(sx.conflicts, sy.conflicts);
  }

  #[test]
  fn huge_penalties_and_many_iterations_cannot_overflow_or_hang() {
    // Regression: the present penalty used to overflow u32 near iteration 50
    // and the search ran away (8 GB allocation on m1 at 0.2 mm rules).
    let (b, m) = setup();
    let p = Params {
      max_iterations: 200,
      present_first: u32::MAX,
      present_growth: (3, 1),
      history_step: u32::MAX,
    };
    let (n, st) = route_negotiated(&b, &m, Costs::default(), p);
    assert!(judge(BOARD, &b, &m, &n.routes).clean());
    assert!(st.iterations >= 1);
  }

  #[test]
  fn zero_iterations_still_returns_a_legal_result() {
    let (b, m) = setup();
    let p = Params {
      max_iterations: 0,
      ..Params::default()
    };
    let (n, _) = route_negotiated(&b, &m, Costs::default(), p);
    assert!(judge(BOARD, &b, &m, &n.routes).clean());
  }

  /// Regression (2026-10-13): plane-tap vias of one net were placed one grid
  /// pitch (0.1778 mm) apart, holes 0.0278 mm apart against a 0.0889 mm rule;
  /// same-net vias were exempt from the via-to-via check. Three pads of one
  /// plane net, so close that each would take the nearest free cell.
  #[test]
  fn plane_taps_of_one_net_keep_hole_to_hole_spacing() {
    let src = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (4 "In1.Cu" power) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "G")
  (gr_line (start 0 0) (end 10 0) (layer "Edge.Cuts"))
  (gr_line (start 10 0) (end 10 6) (layer "Edge.Cuts"))
  (gr_line (start 10 6) (end 0 6) (layer "Edge.Cuts"))
  (gr_line (start 0 6) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 4 3) (size 0.3 0.3) (layers "F.Cu") (net 1 "G"))
    (pad "2" smd rect (at 4.4 3) (size 0.3 0.3) (layers "F.Cu") (net 1 "G"))
    (pad "3" smd rect (at 4.8 3) (size 0.3 0.3) (layers "F.Cu") (net 1 "G")))
)"#;
    let doc = parse(src).unwrap();
    let b = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &b, &r, &p, &[("In1.Cu".to_string(), "G".to_string())]).unwrap();
    let s = route_all(&b, &m, Costs::default());
    let g = s.routes.iter().find(|r| r.net == 1).unwrap();
    assert!(g.vias.len() >= 2, "expected several taps, got {:?}", g.vias);
    for (i, a) in g.vias.iter().enumerate() {
      for c in &g.vias[i + 1..] {
        let d = (m.centre(a.0, a.1) - m.centre(c.0, c.1)).length();
        assert!(
          d >= m.via_spacing - 1e-9,
          "vias {a:?} {c:?} are {d} apart, need {}",
          m.via_spacing
        );
      }
    }
    let v = judge(src, &b, &m, &s.routes);
    assert_eq!(v.hole_to_hole, 0, "{v:?}");
  }
}
