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
//! - **Flip** ([`flip`]) moves a footprint to the other side, about its own
//!   position, the way KiCad's own left-right flip does. Every rule below was
//!   read off files KiCad wrote itself (tools/kicad_flip_probe.py; a whole
//!   board flipped by pcbnew, 2026-10-12), not assumed:
//!   - every `F.x` layer name becomes `B.x` and back; `*.Cu`, `F&B.Cu` stay;
//!   - every local coordinate's y is negated (pads, graphics, texts, polygon
//!     points, arcs); the 3D `model` block is left alone, as KiCad leaves it;
//!   - footprint angle θ → 180 − θ (135→45, 30→150, 0→180 measured);
//!   - pads keep board-frame angles: new = new_fp − (old − old_fp);
//!   - text angle → upright(180 − t), where upright subtracts 180 from angles
//!     in (90, 270]: this reproduced all 7 (footprint, text) angle classes of
//!     366 texts on the m1 fab board; and `(justify mirror)` toggles.

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

/// Move the footprint at root item `node_index` to the other side of the
/// board, about its own position (see the module notes for every rule).
pub fn flip(doc: &mut Document, node_index: usize) -> Result<(), EditError> {
  let root = doc.root_mut().ok_or(EditError::NoRoot)?;
  let fp = root
    .items
    .get_mut(node_index)
    .and_then(Node::as_list_mut)
    .filter(|l| l.is("footprint"))
    .ok_or(EditError::NotAFootprint(node_index))?;
  let old_fp = fp.child("at").and_then(|a| a.number(3)).unwrap_or(0.0);
  let new_fp = normalise(180.0 - old_fp);

  for child in fp.items.iter_mut().filter_map(Node::as_list_mut) {
    let head = child.head().map(|h| h.into_owned()).unwrap_or_default();
    match head.as_str() {
      // The footprint's own position: angle only, the point does not move.
      "at" => set_angle(child, new_fp),
      "layer" | "layers" => swap_layers(child),
      "model" => {}
      "pad" => {
        if let Some(at) = child.children_mut("at").next() {
          negate(at, 2);
          let a = at.number(3).unwrap_or(0.0);
          set_angle(at, new_fp - (a - old_fp));
        }
        mirror_within(child, true);
      }
      "property" | "fp_text" => {
        if let Some(at) = child.children_mut("at").next() {
          negate(at, 2);
          let t = at.number(3).unwrap_or(0.0);
          set_angle(at, upright(180.0 - t));
        }
        toggle_justify_mirror(child);
        mirror_within(child, true);
      }
      _ => {
        mirror_coords(child);
        mirror_within(child, false);
      }
    }
  }
  Ok(())
}

/// Recurse into `list`'s children: swap layer names, negate y of coordinate
/// lists, leave `model` alone. `skip_at` keeps an already-handled `(at …)`.
fn mirror_within(list: &mut List, skip_at: bool) {
  for child in list.items.iter_mut().filter_map(Node::as_list_mut) {
    let head = child.head().map(|h| h.into_owned()).unwrap_or_default();
    match head.as_str() {
      "model" => {}
      "at" if skip_at => {}
      "layer" | "layers" => swap_layers(child),
      _ => {
        mirror_coords(child);
        mirror_within(child, false);
      }
    }
  }
}

/// Negate the y of a coordinate list: `(start x y)`, `(end …)`, `(mid …)`,
/// `(center …)`, `(xy …)`, and a nested `(at …)` / `(offset …)`.
fn mirror_coords(list: &mut List) {
  if ["start", "end", "mid", "center", "xy", "at", "offset"]
    .iter()
    .any(|k| list.is(k))
  {
    negate(list, 2);
  }
}

fn negate(list: &mut List, i: usize) {
  if let Some(v) = list.number(i) {
    set_number(list, i, -v);
  }
}

/// `F.x` ↔ `B.x` for every layer atom in a `(layer …)` / `(layers …)` list.
fn swap_layers(list: &mut List) {
  for node in list.items.iter_mut().skip(1) {
    let Node::Atom(atom) = node else { continue };
    let value = atom.value().into_owned();
    let swapped = if let Some(rest) = value.strip_prefix("F.") {
      format!("B.{rest}")
    } else if let Some(rest) = value.strip_prefix("B.") {
      format!("F.{rest}")
    } else {
      continue;
    };
    let lead = atom.lead.clone();
    *atom = if atom.is_quoted() {
      Atom::quoted(lead, &swapped)
    } else {
      match Atom::bare(lead.clone(), &swapped) {
        Some(a) => a,
        None => Atom::quoted(lead, &swapped),
      }
    };
  }
}

/// Toggle `mirror` in a text's `(effects … (justify …))`, adding or removing
/// the `justify` list as needed.
fn toggle_justify_mirror(text: &mut List) {
  let Some(effects) = text.children_mut("effects").next() else {
    return;
  };
  let lead = effects
    .items
    .iter()
    .filter_map(Node::as_list)
    .next()
    .map_or_else(|| " ".to_owned(), |l| l.lead.clone());
  let had_justify = match effects.children_mut("justify").next() {
    Some(justify) => {
      let had = justify.items.len();
      justify
        .items
        .retain(|n| n.as_atom().is_none_or(|a| a.value() != "mirror"));
      if justify.items.len() == had
        && let Some(a) = Atom::bare(" ", "mirror")
      {
        justify.items.push(Node::Atom(a));
      }
      true
    }
    None => false,
  };
  if had_justify {
    // A justify with nothing left but its keyword means nothing; drop it.
    effects.items.retain(|n| {
      n.as_list()
        .is_none_or(|l| !(l.is("justify") && l.items.len() == 1))
    });
    return;
  }
  if let (Some(k), Some(m)) = (Atom::bare("", "justify"), Atom::bare(" ", "mirror")) {
    effects.items.push(Node::List(List {
      lead,
      items: vec![Node::Atom(k), Node::Atom(m)],
      tail: String::new(),
    }));
  }
}

/// Keep text readable: angles in (90, 270] turn by 180.
fn upright(degrees: f64) -> f64 {
  let d = normalise(degrees);
  if d > 90.0 && d <= 270.0 {
    normalise(d - 180.0)
  } else {
    d
  }
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
