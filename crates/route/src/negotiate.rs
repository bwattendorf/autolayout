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
  /// After the hard re-route of dropped nets, rip up the kept nets that sit in
  /// the landing area of a pad that still failed, route the failed net first
  /// and the ripped ones after it; kept only if fewer pads fail.
  pub ripup_retry: bool,
}

impl Default for Params {
  fn default() -> Params {
    Params {
      max_iterations: 60,
      present_first: 30,
      present_growth: (3, 2),
      history_step: 20,
      ripup_retry: true,
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
  /// Failed nets that the rip-up retry completed or improved (fewer failed pads).
  pub ripup_improved: usize,
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
  if params.ripup_retry {
    stats.ripup_improved = ripup_retry(board, model, costs, &by_net, &nets, &mut kept);
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

/// Most nets ripped up for one retry, and most retries per board.
const RIPUP_MAX_NETS: usize = 8;
const RIPUP_MAX_TRIES: usize = 40;

fn failed_pads(r: &Option<NetRoute>) -> usize {
  r.as_ref().map_or(0, |r| r.failed.len())
}

/// A hard-mode router holding the plane taps and every kept route except `skip`.
fn rebuilt_router<'a>(
  board: &Board,
  model: &'a RoutingModel,
  costs: Costs,
  by_net: &std::collections::HashMap<u32, Vec<(usize, usize)>>,
  kept: &[Option<NetRoute>],
  skip: &[usize],
) -> Router<'a> {
  let mut router = Router::new(model, costs);
  let mut scratch = Summary::default();
  tap_planes(board, model, &mut router, by_net, &mut scratch);
  for (j, r) in kept.iter().enumerate() {
    if let (Some(r), false) = (r, skip.contains(&j)) {
      router.commit_hard(r);
    }
  }
  router
}

/// Rip-up and retry for nets still incomplete after the hard re-route. A failed
/// pad usually lost its landing cells to the copper of nets routed before it,
/// so for each failed net the kept nets whose copper lies within the stub reach
/// of a failed pad are ripped up, the failed net is routed first, and the
/// ripped nets after it. The trial is kept only when it leaves fewer failed
/// pads over those nets; otherwise nothing changes. Net order is fixed, so the
/// result is deterministic. Returns how many failed nets improved.
fn ripup_retry(
  board: &Board,
  model: &RoutingModel,
  costs: Costs,
  by_net: &std::collections::HashMap<u32, Vec<(usize, usize)>>,
  nets: &[crate::driver::Ordered],
  kept: &mut [Option<NetRoute>],
) -> usize {
  let mut improved = 0usize;
  let mut tries = 0usize;
  for k in 0..nets.len() {
    if tries >= RIPUP_MAX_TRIES {
      break;
    }
    let Some(failed) = kept.get(k).and_then(|r| r.as_ref()).map(|r| r.failed.clone()) else {
      continue;
    };
    if failed.is_empty() {
      continue;
    }
    // Kept nets with copper near a failed pad, on the pad's own layers.
    let mut rip: Vec<usize> = Vec::new();
    for &(fi, pi) in &failed {
      let Some(pad) = board.footprints.get(fi).and_then(|f| f.pads.get(pi)) else {
        continue;
      };
      let reach = model.pitch * costs.reach + pad.size.x.max(pad.size.y) / 2.0;
      let layers = model.pad_layer_indices(&pad.layers);
      for (j, r) in kept.iter().enumerate() {
        let Some(r) = r else { continue };
        if j == k || rip.contains(&j) {
          continue;
        }
        let near = r
          .cells
          .iter()
          .chain(r.stub_cells.iter())
          .any(|&(l, x, y)| layers.contains(&l) && (model.centre(x, y) - pad.at).length() <= reach);
        if near {
          rip.push(j);
        }
      }
    }
    rip.sort_unstable();
    if rip.is_empty() || rip.len() > RIPUP_MAX_NETS {
      continue;
    }
    tries += 1;
    let mut skip = rip.clone();
    skip.push(k);
    let before: usize = skip.iter().map(|&j| failed_pads(&kept[j])).sum();
    let mut router = rebuilt_router(board, model, costs, by_net, kept, &skip);
    let mut trial: Vec<(usize, NetRoute)> = Vec::new();
    for &j in std::iter::once(&k).chain(rip.iter()) {
      if let Some((_, net, pads)) = nets.get(j) {
        trial.push((j, router.route_net(board, *net, pads)));
      }
    }
    let after: usize = trial.iter().map(|(_, r)| r.failed.len()).sum();
    if after < before {
      for (j, r) in trial {
        if let Some(slot) = kept.get_mut(j) {
          *slot = Some(r);
        }
      }
      improved += 1;
    }
  }
  improved
}
