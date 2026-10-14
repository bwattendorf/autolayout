//! Negotiated congestion routing (M7.3): PathFinder.
//!
//! McMurchie & Ebeling, "PathFinder: A Negotiation-Based Performance-Driven
//! Router for FPGAs", FPGA '95 (ACM). Every net is routed as if it were alone
//! except that a cell already held by another net costs more: its *present*
//! penalty, which grows each iteration, plus its *history*, which grows every
//! iteration the cell is still contested. Nets whose copper sits on a
//! contested cell are ripped up and rerouted; cheap detours win out over
//! fighting for a cell, and the nets settle. Iterations stop at no conflict
//! or at the cap. Net order, costs and tie-breaks are fixed, so the result is
//! deterministic (CLAUDE.md rule 4).
//!
//! If conflicts remain at the cap, the best iteration seen (fewest conflicts)
//! is kept and made legal by dropping, in net order, every net that shares a
//! cell with one already kept. Dropped nets are reported as unrouted, never
//! hidden.

use autolayout_board::Board;

use crate::driver::{Summary, order_signal_nets, tap_planes};
use crate::model::RoutingModel;
use crate::search::{Costs, NetRoute, Router};

#[derive(Debug, Clone, Copy)]
pub struct Params {
  pub max_iterations: usize,
  /// First present-sharing penalty, in cost units (one grid step is 10).
  pub present_first: u32,
  /// Present penalty grows by `num / den` each iteration.
  pub present_growth: (u32, u32),
  /// History added to a contested cell each iteration.
  pub history_step: u32,
}

impl Default for Params {
  fn default() -> Params {
    Params {
      max_iterations: 60,
      present_first: 30,
      present_growth: (3, 2),
      history_step: 20,
    }
  }
}

#[derive(Debug, Clone, Default)]
pub struct NegotiationStats {
  pub iterations: usize,
  /// Contested cells after each iteration (index 0 = first full pass).
  pub conflicts: Vec<usize>,
  pub dropped_nets: usize,
  /// Of the dropped nets, how many came out complete when re-routed in hard
  /// mode around the nets that were kept.
  pub repaired_nets: usize,
}

pub fn route_negotiated(
  board: &Board,
  model: &RoutingModel,
  costs: Costs,
  params: Params,
) -> (Summary, NegotiationStats) {
  let mut router = Router::new(model, costs);
  let by_net = board.pads_by_net();
  let mut s = Summary::default();
  tap_planes(board, model, &mut router, &by_net, &mut s);
  let nets = order_signal_nets(board, model, &by_net);

  router.enable_soft(params.present_first);
  let mut routes: Vec<NetRoute> = nets
    .iter()
    .map(|(_, n, _)| NetRoute {
      net: *n,
      ..NetRoute::default()
    })
    .collect();
  let mut fps: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nets.len()];
  let mut fpss: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nets.len()];
  let mut stats = NegotiationStats::default();
  let mut best: Option<(usize, Vec<NetRoute>)> = None;
  let mut stuck = 0usize;

  for it in 0..=params.max_iterations {
    if it > 0 {
      // A count that has stopped falling means two nets are locked over the
      // same cell; raise the price of that cell faster the longer it lasts.
      let mult = 1 + (stuck / 3).min(9) as u32;
      router.bump_history(params.history_step.saturating_mul(mult));
      router.grow_present(params.present_growth.0, params.present_growth.1);
    }
    for (k, (_, net, pads)) in nets.iter().enumerate() {
      let again = it == 0
        || router.is_contested(
          fps.get(k).map_or(&[][..], Vec::as_slice),
          fpss.get(k).map_or(&[][..], Vec::as_slice),
        );
      if !again {
        continue;
      }
      if let Some(fp) = fps.get(k) {
        router.occupy(fp, false);
      }
      if let Some(fp) = fpss.get(k) {
        router.occupy_s(fp, false);
      }
      let r = router.route_net(board, *net, pads);
      let fp = router.footprint(&r);
      let fs = router.footprint_s(&r);
      router.occupy(&fp, true);
      router.occupy_s(&fs, true);
      if let (Some(slot), Some(f), Some(g)) = (routes.get_mut(k), fps.get_mut(k), fpss.get_mut(k)) {
        *slot = r;
        *f = fp;
        *g = fs;
      }
    }
    let c = router.conflicts();
    stuck = if stats.conflicts.last() == Some(&c) {
      stuck + 1
    } else {
      0
    };
    stats.conflicts.push(c);
    stats.iterations = it + 1;
    if best.as_ref().is_none_or(|(bc, _)| c < *bc) {
      best = Some((c, routes.clone()));
    }
    if c == 0 {
      break;
    }
  }

  // Legalise the best iteration: keep a net only if it shares no cell with one
  // already kept. A dropped net is not given up on: it is re-routed in hard
  // mode around everything kept (before this, a single contested cell that
  // survived the iterations cost a net all of its pads: m1's N$4, round 7).
  let (_, best_routes) = best.unwrap_or((0, routes));
  let mut taken: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
  let mut taken_s: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
  let mut kept: Vec<Option<NetRoute>> = Vec::new();
  for r in best_routes {
    let fp = router.footprint(&r);
    let fs = router.footprint_s(&r);
    if fp.iter().any(|c| taken.contains(c) || taken_s.contains(c)) || fs.iter().any(|c| taken.contains(c)) {
      stats.dropped_nets += 1;
      kept.push(None);
    } else {
      taken.extend(fp);
      taken_s.extend(fs);
      kept.push(Some(r));
    }
  }
  router.leave_soft();
  for r in kept.iter().flatten() {
    router.commit_hard(r);
  }
  for (slot, (_, net, pads)) in kept.iter_mut().zip(&nets) {
    if slot.is_none() {
      let r = router.route_net(board, *net, pads);
      if r.failed.is_empty() {
        stats.repaired_nets += 1;
      }
      *slot = Some(r);
    }
  }
  for ((_, _, pads), r) in nets.iter().zip(kept) {
    let Some(kept) = r else { continue };
    s.nets_total += 1;
    s.pads_total += pads.len();
    s.pads_connected += kept.connected.len();
    if kept.failed.is_empty() {
      s.nets_complete += 1;
    }
    s.length_mm += kept.length_mm(model.pitch);
    s.vias += kept.vias.len();
    s.routes.push(kept);
  }
  (s, stats)
}
