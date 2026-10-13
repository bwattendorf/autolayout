//! Choosing which nets get the inner plane layers, from the board alone.
//!
//! A board file does not say which layers are planes once its zones are
//! stripped, and a netlist is not always at hand, so the plan is structural
//! (CLAUDE.md rule 2): the nets with the most pads, at least
//! [`PLANE_MIN_PADS`], take the inner copper layers in stack order, most pads
//! first. No net name is consulted; names appear only in the output. This is
//! the same "more than 16 pads is a plane net" rule `autolayout grade` uses.
//! It is a plan, not a fact about the design: a board whose second-largest
//! net is not a supply would be planned wrongly, which is why the plan is
//! printed with the pad counts behind it.

use autolayout_board::Board;
use autolayout_sexpr::Document;

/// Fewest pads for a net to be given a plane (same threshold as the grader).
pub const PLANE_MIN_PADS: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanePlan {
  /// Every copper layer, outermost first.
  pub copper: Vec<String>,
  /// `(layer, net name, pad count)` for each inner layer given a plane.
  pub assignments: Vec<(String, String, usize)>,
  /// Inner layers left without a plane for want of a big enough net.
  pub unassigned: Vec<String>,
}

pub fn plan(doc: &Document, board: &Board) -> PlanePlan {
  let copper: Vec<String> = doc
    .root()
    .and_then(|r| r.child("layers"))
    .map(|l| {
      l.items
        .iter()
        .filter_map(|n| n.as_list())
        .filter_map(|e| e.atom(1))
        .filter(|n| n.ends_with(".Cu"))
        .map(|n| n.into_owned())
        .collect()
    })
    .unwrap_or_default();
  let inner: Vec<String> = copper
    .iter()
    .filter(|l| l.as_str() != "F.Cu" && l.as_str() != "B.Cu")
    .cloned()
    .collect();
  let mut nets: Vec<(usize, u32)> = board
    .pads_by_net()
    .into_iter()
    .filter(|(n, pads)| *n != 0 && pads.len() >= PLANE_MIN_PADS)
    .map(|(n, pads)| (pads.len(), n))
    .collect();
  // Most pads first; ties by net code so the plan is deterministic.
  nets.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
  let mut assignments = Vec::new();
  let mut unassigned = Vec::new();
  for (i, layer) in inner.iter().enumerate() {
    match nets.get(i) {
      Some(&(count, net)) => assignments.push((
        layer.clone(),
        board.nets.get(&net).cloned().unwrap_or_default(),
        count,
      )),
      None => unassigned.push(layer.clone()),
    }
  }
  PlanePlan {
    copper,
    assignments,
    unassigned,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use autolayout_sexpr::parse;

  fn board(extra_pads: usize) -> String {
    let mut pads = String::new();
    // Net 1 gets 20 pads, net 2 gets `extra_pads`, net 3 gets 3.
    for i in 0..20 {
      pads += &format!("(pad \"{i}\" smd rect (at {i} 1) (size 1 1) (layers \"F.Cu\") (net 1 \"A\"))");
    }
    for i in 0..extra_pads {
      pads += &format!("(pad \"{i}\" smd rect (at {i} 3) (size 1 1) (layers \"F.Cu\") (net 2 \"B\"))");
    }
    for i in 0..3 {
      pads += &format!("(pad \"{i}\" smd rect (at {i} 5) (size 1 1) (layers \"F.Cu\") (net 3 \"C\"))");
    }
    format!(
      r#"(kicad_pcb (layers (0 "F.Cu" signal) (4 "In1.Cu" power) (6 "In2.Cu" power) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C")
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1") {pads}))"#
    )
  }

  #[test]
  fn biggest_nets_take_the_inner_layers_in_order() {
    let doc = parse(&board(30)).unwrap();
    let p = plan(&doc, &Board::read(&doc).unwrap());
    assert_eq!(
      p.assignments,
      vec![
        ("In1.Cu".into(), "B".into(), 30),
        ("In2.Cu".into(), "A".into(), 20)
      ]
    );
    assert!(p.unassigned.is_empty());
  }

  #[test]
  fn a_layer_without_a_big_enough_net_is_left_alone() {
    let doc = parse(&board(5)).unwrap();
    let p = plan(&doc, &Board::read(&doc).unwrap());
    assert_eq!(p.assignments.len(), 1);
    assert_eq!(p.unassigned, vec!["In2.Cu".to_string()]);
  }

  #[test]
  fn a_two_layer_board_has_no_inner_layers() {
    let doc = parse(&board(30).replace("(4 \"In1.Cu\" power) (6 \"In2.Cu\" power) ", "")).unwrap();
    let p = plan(&doc, &Board::read(&doc).unwrap());
    assert!(p.assignments.is_empty() && p.unassigned.is_empty());
  }
}
