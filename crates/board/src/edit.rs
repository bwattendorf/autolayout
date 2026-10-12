//! Footprint edits that change only the tokens they mean to change.
//!
//! What a pose change touches, and why (both measured on the corpus, see
//! `tests/corpus.rs`):
//! - **Move** rewrites the footprint's own `(at x y …)` and nothing else. Pads,
//!   texts and graphics are stored in the footprint's frame, so they follow.
//! - **Rotate** rewrites the footprint's angle *and* adds the same delta to the
//!   angle of every pad, property and text inside it, because KiCad stores
//!   those angles in board terms (7072/7072 rotated chip pads read exactly
//!   their footprint's rotation). Graphics (`fp_line` …) carry no angle and
//!   are untouched.
//!
//! Flipping to the other side is not here: it mirrors coordinates and swaps
//! every layer name, and is its own task (docs/gaps.md).

use autolayout_sexpr::{Atom, Document, List, Node};

use crate::Point;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
  NoRoot,
  NotAFootprint(usize),
  NonFinite,
}

impl std::fmt::Display for EditError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      EditError::NoRoot => f.write_str("document has no root list"),
      EditError::NotAFootprint(i) => write!(f, "root item {i} is not a footprint"),
      EditError::NonFinite => f.write_str("position or angle is not a finite number"),
    }
  }
}

impl std::error::Error for EditError {}

/// Set the footprint at root item `node_index` to position `at` and
/// orientation `rotation` degrees.
pub fn set_pose(doc: &mut Document, node_index: usize, at: Point, rotation: f64) -> Result<(), EditError> {
  if !(at.x.is_finite() && at.y.is_finite() && rotation.is_finite()) {
    return Err(EditError::NonFinite);
  }
  let root = doc.root_mut().ok_or(EditError::NoRoot)?;
  let fp = root
    .items
    .get_mut(node_index)
    .and_then(Node::as_list_mut)
    .filter(|l| l.is("footprint"))
    .ok_or(EditError::NotAFootprint(node_index))?;

  let old_rotation = fp.child("at").and_then(|a| a.number(3)).unwrap_or(0.0);
  let delta = rotation - old_rotation;

  if let Some(at_list) = fp.children_mut("at").next() {
    set_number(at_list, 1, at.x);
    set_number(at_list, 2, at.y);
    set_angle(at_list, rotation);
  }
  if delta.abs() > 1e-9 {
    for child in fp.items.iter_mut().filter_map(Node::as_list_mut) {
      let carries_angle = child.is("pad") || child.is("property") || child.is("fp_text");
      if !carries_angle {
        continue;
      }
      if let Some(at_list) = child.children_mut("at").next() {
        let angle = at_list.number(3).unwrap_or(0.0);
        set_angle(at_list, angle + delta);
      }
    }
  }
  Ok(())
}

/// Write `value` as item `i`, keeping the whitespace that preceded the old one.
fn set_number(list: &mut List, i: usize, value: f64) {
  let text = format_number(value);
  match list.items.get_mut(i) {
    Some(Node::Atom(old)) => {
      if let Some(atom) = Atom::bare(old.lead.clone(), &text) {
        *old = atom;
      }
    }
    Some(Node::List(_)) => {}
    None => {
      if let Some(atom) = Atom::bare(" ", &text) {
        list.items.push(Node::Atom(atom));
      }
    }
  }
}

/// Set the angle slot (item 3) of an `(at x y [angle])`. KiCad omits the
/// angle when it is zero; we keep an existing slot and only add one when the
/// new angle is non-zero.
fn set_angle(at: &mut List, degrees: f64) {
  let degrees = normalise(degrees);
  if at.items.len() > 3 || degrees != 0.0 {
    set_number(at, 3, degrees);
  }
}

/// Into [0, 360), with values within 1e-9 of a whole degree snapped to it.
fn normalise(degrees: f64) -> f64 {
  let d = degrees.rem_euclid(360.0);
  let snapped = d.round();
  let d = if (d - snapped).abs() < 1e-9 { snapped } else { d };
  if d >= 360.0 { 0.0 } else { d }
}

/// KiCad's style: at most 6 decimals, no trailing zeros, no `-0`.
pub fn format_number(value: f64) -> String {
  let s = format!("{value:.6}");
  let s = s.trim_end_matches('0').trim_end_matches('.');
  if s == "-0" || s.is_empty() {
    "0".to_owned()
  } else {
    s.to_owned()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use autolayout_sexpr::parse;

  const FP: &str = "(kicad_pcb\n\t(footprint \"R\"\n\t\t(layer \"F.Cu\")\n\t\t(at 10 20 90)\n\t\t(property \"Reference\" \"R1\"\n\t\t\t(at 0 2 270)\n\t\t)\n\t\t(fp_line\n\t\t\t(start -1 0)\n\t\t\t(end 1 0)\n\t\t)\n\t\t(pad \"1\" smd rect\n\t\t\t(at -0.8 0 90)\n\t\t)\n\t\t(pad \"2\" smd rect\n\t\t\t(at 0.8 0)\n\t\t)\n\t)\n)\n";

  mod pose {
    use super::*;

    /// Root item 0 is the `kicad_pcb` keyword; the footprint is item 1, the
    /// same numbering `Board::read` records in `node_index`.
    const FP_INDEX: usize = 1;

    #[test]
    fn move_only_touches_footprint_at() {
      let mut doc = parse(FP).unwrap();
      set_pose(&mut doc, FP_INDEX, Point::new(12.5, -3.25), 90.0).unwrap();
      let expected = FP.replace("(at 10 20 90)", "(at 12.5 -3.25 90)");
      assert_eq!(doc.to_string(), expected);
    }

    #[test]
    fn rotate_adds_delta_to_pads_and_texts_only() {
      let mut doc = parse(FP).unwrap();
      set_pose(&mut doc, FP_INDEX, Point::new(10.0, 20.0), 180.0).unwrap();
      let expected = FP
        .replace("(at 10 20 90)", "(at 10 20 180)")
        .replace("(at 0 2 270)", "(at 0 2 0)")
        .replace("(at -0.8 0 90)", "(at -0.8 0 180)")
        .replace("(at 0.8 0)", "(at 0.8 0 90)");
      assert_eq!(doc.to_string(), expected);
    }

    #[test]
    fn bad_input_is_an_error() {
      let mut doc = parse(FP).unwrap();
      assert_eq!(
        set_pose(&mut doc, 5, Point::new(0.0, 0.0), 0.0),
        Err(EditError::NotAFootprint(5))
      );
      assert_eq!(
        set_pose(&mut doc, FP_INDEX, Point::new(f64::NAN, 0.0), 0.0),
        Err(EditError::NonFinite)
      );
      assert_eq!(doc.to_string(), FP, "a failed edit must not change the document");
    }
  }

  mod numbers {
    use super::*;

    #[test]
    fn formatting_matches_kicad_style() {
      for (v, s) in [
        (114.0, "114"),
        (-0.825, "-0.825"),
        (74.916575, "74.916575"),
        (-0.0000001, "0"),
        (1e-7, "0"),
      ] {
        assert_eq!(format_number(v), s, "{v}");
      }
    }

    #[test]
    fn angles_normalise_into_range() {
      assert_eq!(normalise(-90.0), 270.0);
      assert_eq!(normalise(360.0), 0.0);
      assert_eq!(normalise(450.0000000001), 90.0);
    }
  }
}
