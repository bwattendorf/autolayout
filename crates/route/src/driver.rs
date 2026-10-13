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
type Ordered = (f64, u32, Vec<(usize, usize)>);

/// Half-perimeter of a net's pad box, used to order nets short-first.
fn hpwl(board: &Board, pads: &[(usize, usize)]) -> f64 {
  let pts = pads
    .iter()
    .filter_map(|&(f, p)| board.footprints.get(f)?.pads.get(p).map(|p| p.at));
  autolayout_board::BBox::from_points(pts).map_or(0.0, |b| b.width() + b.height())
}

pub fn route_all(board: &Board, model: &RoutingModel, costs: Costs) -> Summary {
  let mut router = Router::new(model, costs);
  let by_net = board.pads_by_net();
  let plane_nets: Vec<u32> = model.planes.iter().map(|p| p.net).collect();
  let mut s = Summary::default();

  let mut codes: Vec<u32> = by_net.keys().copied().filter(|&n| n != 0).collect();
  codes.sort_unstable();

  // Plane nets: one via per pad that does not already reach the plane.
  for &net in codes.iter().filter(|n| plane_nets.contains(n)) {
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
    if agg.failed.is_empty() {
      s.nets_complete += 1;
    }
    s.routes.push(agg);
  }

  let mut signal: Vec<Ordered> = codes
    .iter()
    .filter(|n| !plane_nets.contains(n))
    .filter_map(|&n| {
      let mut pads = by_net.get(&n)?.clone();
      pads.sort_unstable();
      (pads.len() >= 2).then(|| (hpwl(board, &pads), n, pads))
    })
    .collect();
  signal.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
  for (_, net, pads) in signal {
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
  s.vias += s
    .routes
    .iter()
    .take_while(|r| plane_nets.contains(&r.net))
    .map(|r| r.vias.len())
    .sum::<usize>();
  s
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::check::check;
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
    let c = check(&b, &m, &s.routes);
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
