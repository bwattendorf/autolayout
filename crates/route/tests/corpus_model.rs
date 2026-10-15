//! Checks on a real board. Skip, printing why, without the corpus.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use autolayout_board::Board;
use autolayout_route::driver::route_all;
use autolayout_route::model::RoutingModel;
use autolayout_route::negotiate::{Params, route_negotiated};
use autolayout_route::rules::DesignRules;
use autolayout_route::search::Costs;
use autolayout_route::verify::verify;
use autolayout_route::write::add_routes;
use autolayout_sexpr::{Document, parse};

/// The m1 fab board with its copper removed (what `autolayout strip` does).
fn stripped_fab() -> Option<(Document, Board, DesignRules)> {
  let root = std::env::var("AUTOLAYOUT_CORPUS").ok()?;
  let path = Path::new(&root).join("quilter/fab/m1/m1.kicad_pcb");
  let mut doc = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
  let copper = ["segment", "arc", "via", "zone"];
  doc
    .root_mut()
    .unwrap()
    .items
    .retain(|n| n.as_list().is_none_or(|l| !copper.iter().any(|k| l.is(k))));
  let board = Board::read(&doc).unwrap();
  let rules = DesignRules::load_for_board(&path).unwrap();
  Some((doc, board, rules))
}

fn planes() -> Vec<(String, String)> {
  vec![
    ("In1.Cu".to_string(), "GND".to_string()),
    ("In2.Cu".to_string(), "+3V3".to_string()),
  ]
}

/// m1 at its fab rules, routed and written, then judged from the written text
/// alone. Measured 2026-10-13: 104 of 104 nets, verifier 0 in every category.
/// (Before the pad-geometry, via-spacing and stub fixes the same board gave
/// 2 unconnected + 2 dangling + 2 hole_to_hole in KiCad.) The verifier is our
/// code; KiCad DRC is the judge (docs/m7-results).
#[test]
fn m1_fab_routes_and_the_verifier_is_clean() {
  let Some((doc, board, rules)) = stripped_fab() else {
    println!("SKIPPED: route corpus test (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let profile = rules.classes[0].clone();
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes()).unwrap();
  let (s, st) = route_negotiated(&board, &m, Costs::default(), Params::default());
  assert_eq!(st.conflicts.last(), Some(&0), "{:?}", st.conflicts);
  assert_eq!(s.nets_complete, s.nets_total, "incomplete nets");
  assert!(
    s.nets_total > 100,
    "too few nets ({}); refusing a vacuous pass",
    s.nets_total
  );

  let mut out = doc.clone();
  let w = add_routes(&mut out, &m, &board, &s).unwrap();
  let text = out.to_string();
  let back = parse(&text).unwrap();
  assert_eq!(back.to_string(), text);
  let b2 = Board::read(&back).unwrap();
  let codes: Vec<(String, u32)> = m.planes.iter().map(|p| (p.layer.clone(), p.net)).collect();
  let v = verify(&back, &b2, &rules, &codes);
  println!(
    "route corpus: m1 fab {} nets, {} segments, {} vias; verifier: {}",
    s.nets_total,
    w.segments,
    w.vias,
    v.summary()
  );
  assert!(
    v.clean(),
    "{}\n{:?}",
    v.summary(),
    &v.issues[..v.issues.len().min(10)]
  );
  assert_eq!((v.segments, v.vias), (w.segments, w.vias));
}

/// The greedy baseline writes a board the verifier can also read (it need not
/// be clean: greedy leaves nets unfinished on purpose).
#[test]
fn m1_fab_greedy_is_deterministic() {
  let Some((doc, board, rules)) = stripped_fab() else {
    println!("SKIPPED: route corpus test (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let profile = rules.classes[0].clone();
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes()).unwrap();
  let a = route_all(&board, &m, Costs::default());
  let b = route_all(&board, &m, Costs::default());
  assert_eq!(a.routes, b.routes);
}

/// A service candidate with a crowded MCU: before the hard-mode repair of
/// dropped nets this routed 87 of 99 nets (KiCad and the verifier agreed on 34
/// unconnected); with it, 89 of 99 and 29 unconnected (2026-10-13, local run,
/// measured). The repair must complete at least one net the negotiation gave up.
#[test]
fn dropped_nets_are_repaired_where_possible() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    println!("SKIPPED: route corpus test (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let path = Path::new(&root).join("quilter/returned/m1/candidate4/m1.kicad_pcb");
  let mut doc = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
  let copper = ["segment", "arc", "via", "zone"];
  doc
    .root_mut()
    .unwrap()
    .items
    .retain(|n| n.as_list().is_none_or(|l| !copper.iter().any(|k| l.is(k))));
  let board = Board::read(&doc).unwrap();
  let rules = DesignRules::load_for_board(&path).unwrap();
  let profile = rules.classes[0].clone();
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes()).unwrap();
  // Stub-corridor sharing off: with it on (the default since round 19) the first
  // pass drops fewer nets and the hard re-route has nothing left to mend on
  // this board, so the repair is exercised in the mode where it is needed.
  let costs = Costs {
    share_stubs: false,
    ..Costs::default()
  };
  let (s, st) = route_negotiated(&board, &m, costs, Params::default());
  // The floor from 2026-10-13 holds for the default settings of one attempt.
  let (d, _) = route_negotiated(&board, &m, Costs::default(), Params::default());
  assert!(d.nets_complete >= 89, "{} of {}", d.nets_complete, d.nets_total);
  println!(
    "route corpus: m1 candidate4 {} of {} nets, {} dropped, {} repaired",
    s.nets_complete, s.nets_total, st.dropped_nets, st.repaired_nets
  );
  assert!(
    st.dropped_nets > 0,
    "this board is expected to drop nets; the test would be vacuous"
  );
  assert!(
    st.repaired_nets >= 1,
    "no dropped net was completed by the hard re-route"
  );
}
