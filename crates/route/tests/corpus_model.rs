//! Model checks on a real board. Skips, printing why, without the corpus.

use std::path::Path;

use autolayout_board::Board;
use autolayout_route::model::RoutingModel;
use autolayout_route::rules::DesignRules;
use autolayout_sexpr::parse;

/// m1 at its fab rules (pitch 0.178 mm): every netted pad other than the
/// SMD pads of the two plane nets has at least one grid cell inside its copper
/// that its own net may use. Measured 345 of 345 on 2026-10-13.
#[test]
fn m1_fab_every_pad_is_reachable() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    println!("SKIPPED: route model corpus test (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let path = Path::new(&root).join("quilter/fab/m1/m1.kicad_pcb");
  let doc = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
  let board = Board::read(&doc).unwrap();
  let rules = DesignRules::load_for_board(&path).unwrap();
  let profile = rules.classes[0].clone();
  let planes = vec![
    ("In1.Cu".to_string(), "GND".to_string()),
    ("In2.Cu".to_string(), "+3V3".to_string()),
  ];
  let m = RoutingModel::build(&doc, &board, &rules, &profile, &planes).unwrap();
  let plane_nets: Vec<u32> = m.planes.iter().map(|p| p.net).collect();
  let (mut total, mut bad) = (0, 0);
  for (fi, fp) in board.footprints.iter().enumerate() {
    for (pi, pad) in fp.pads.iter().enumerate() {
      let Some(net) = pad.net.filter(|&n| n != 0) else {
        continue;
      };
      if plane_nets.contains(&net) && pad.kind == "smd" {
        continue;
      }
      total += 1;
      if m.pad_access(&board, fi, pi).cells.is_empty() {
        bad += 1;
      }
    }
  }
  println!(
    "route model: m1 fab, {bad} of {total} pads unreachable on the {:.4} mm grid",
    m.pitch
  );
  assert!(
    total > 300,
    "too few pads checked ({total}); refusing a vacuous pass"
  );
  assert_eq!(bad, 0);
}
