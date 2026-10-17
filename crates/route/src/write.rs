//! Writing routes into a board (M7.4), through the lossless layer.
//!
//! Tracks and vias are appended to the root list as KiCad 9 writes them
//! (`(segment (start) (end) (width) (layer) (net) (uuid))`, `(via (at) (size)
//! (drill) (layers) (net) (uuid))`, tab-indented); every existing byte stays
//! where it was. Token layout copied from `quilter/fab/m1/m1.kicad_pcb`, a
//! file KiCad itself wrote. UUIDs are derived from the item's content so the
//! same routes give the same bytes (CLAUDE.md rule 4).
//!
//! **The judge is KiCad DRC**, not this module: it reports what it wrote.

use autolayout_board::edit::format_number;
use autolayout_board::{Board, Point};
use autolayout_sexpr::{Document, Node, parse};

use crate::driver::Summary;
use crate::model::RoutingModel;

#[derive(Debug, Default, PartialEq)]
pub struct Written {
  pub segments: usize,
  pub vias: usize,
}

/// Two FNV-1a 64-bit hashes of the text, shaped like a version-4 UUID.
fn uuid(text: &str) -> String {
  let fnv = |seed: u64| {
    let mut h = seed;
    for b in text.bytes() {
      h ^= u64::from(b);
      h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
  };
  let (a, b) = (fnv(0xcbf2_9ce4_8422_2325), fnv(0x8422_2325_cbf2_9ce4));
  format!(
    "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
    (a >> 32) as u32,
    (a >> 16) as u16,
    (a & 0xfff) as u16,
    0x8000 | ((b >> 48) as u16 & 0x3fff),
    b & 0xffff_ffff_ffff
  )
}

fn xy(p: Point) -> String {
  format!("{} {}", format_number(p.x), format_number(p.y))
}

fn segment(a: Point, b: Point, width: f64, layer: &str, net: u32, net_ref: &str) -> String {
  let key = format!("seg {} {} {layer} {net}", xy(a), xy(b));
  format!(
    "\n\t(segment\n\t\t(start {})\n\t\t(end {})\n\t\t(width {})\n\t\t(layer \"{layer}\")\n\t\t(net {net_ref})\n\t\t(uuid \"{}\")\n\t)",
    xy(a),
    xy(b),
    format_number(width),
    uuid(&key)
  )
}

fn via(at: Point, size: f64, drill: f64, top: &str, bottom: &str, net: u32, net_ref: &str) -> String {
  let key = format!("via {} {net}", xy(at));
  format!(
    "\n\t(via\n\t\t(at {})\n\t\t(size {})\n\t\t(drill {})\n\t\t(layers \"{top}\" \"{bottom}\")\n\t\t(net {net_ref})\n\t\t(uuid \"{}\")\n\t)",
    xy(at),
    format_number(size),
    format_number(drill),
    uuid(&key)
  )
}

/// How the file refers to a net: by code (KiCad 9) or by quoted name (KiCad 10).
fn net_ref_text(board: &Board, net: u32) -> Result<String, String> {
  if board.net_table.by_name_style {
    let name = board
      .nets
      .get(&net)
      .ok_or_else(|| format!("net {net} has no name in this board"))?;
    Ok(format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\"")))
  } else {
    Ok(net.to_string())
  }
}

/// Put generated copper text into the document, before the trailing
/// embedded-fonts/files items where KiCad writes it.
fn insert_copper(doc: &mut Document, text: &str) -> Result<(), String> {
  let added = parse(text).map_err(|e| format!("internal: generated copper does not parse: {e}"))?;
  let root = doc.root_mut().ok_or("no root list")?;
  let at = root
    .items
    .iter()
    .position(|n| {
      n.as_list()
        .is_some_and(|l| l.is("embedded_fonts") || l.is("embedded_files"))
    })
    .unwrap_or(root.items.len());
  let new: Vec<Node> = added
    .items
    .into_iter()
    .filter(|n| matches!(n, Node::List(_)))
    .collect();
  root.items.splice(at..at, new);
  Ok(())
}

/// Append the copper of the fine-lattice router (`fine::route_fine`) to `doc`.
pub fn add_routes_fine(
  doc: &mut Document,
  model: &RoutingModel,
  board: &Board,
  routes: &[crate::fine::FineRoute],
) -> Result<Written, String> {
  let width = model.profile.track_width;
  let (top, bottom) = match (model.stack.first(), model.stack.last()) {
    (Some(t), Some(b)) => (t.clone(), b.clone()),
    _ => return Err("board has no copper layers".to_owned()),
  };
  let mut text = String::new();
  let mut w = Written::default();
  for r in routes {
    let nref = net_ref_text(board, r.net)?;
    for (layer, a, b) in &r.segs {
      let name = model
        .layers
        .get(*layer)
        .ok_or_else(|| format!("route uses layer {layer} the model does not have"))?;
      text += &segment(*a, *b, width, name, r.net, &nref);
      w.segments += 1;
    }
    for &at in &r.vias {
      text += &via(
        at,
        model.profile.via_diameter,
        model.profile.via_drill,
        &top,
        &bottom,
        r.net,
        &nref,
      );
      w.vias += 1;
    }
  }
  if !text.is_empty() {
    insert_copper(doc, &text)?;
  }
  Ok(w)
}

/// Append every route in `summary` to `doc`. Returns what was written.
pub fn add_routes(
  doc: &mut Document,
  model: &RoutingModel,
  board: &Board,
  summary: &Summary,
) -> Result<Written, String> {
  let width = model.profile.track_width;
  let (top, bottom) = match (model.stack.first(), model.stack.last()) {
    (Some(t), Some(b)) => (t.clone(), b.clone()),
    _ => return Err("board has no copper layers".to_owned()),
  };
  let layer_name = |i: usize| {
    model
      .layers
      .get(i)
      .cloned()
      .ok_or_else(|| format!("route uses layer {i} the model does not have"))
  };
  let net_ref = |net: u32| net_ref_text(board, net);
  let mut text = String::new();
  let mut w = Written::default();
  for r in &summary.routes {
    let nref = net_ref(r.net)?;
    for run in &r.runs {
      let (a, b) = (model.centre(run.a.0, run.a.1), model.centre(run.b.0, run.b.1));
      text += &segment(a, b, width, &layer_name(run.layer)?, r.net, &nref);
      w.segments += 1;
    }
    for s in &r.stubs {
      if s.from != s.to {
        text += &segment(s.from, s.to, width, &layer_name(s.layer)?, r.net, &nref);
        w.segments += 1;
      }
    }
    for &(ix, iy) in &r.vias {
      text += &via(
        model.centre(ix, iy),
        model.profile.via_diameter,
        model.profile.via_drill,
        &top,
        &bottom,
        r.net,
        &nref,
      );
      w.vias += 1;
    }
  }
  if text.is_empty() {
    return Ok(w);
  }
  insert_copper(doc, &text)?;
  Ok(w)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::driver::route_all;
  use crate::rules::DesignRules;
  use crate::search::Costs;

  const BOARD: &str = r#"(kicad_pcb
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "A")
  (gr_line (start 0 0) (end 12 0) (layer "Edge.Cuts"))
  (gr_line (start 12 0) (end 12 6) (layer "Edge.Cuts"))
  (gr_line (start 12 6) (end 0 6) (layer "Edge.Cuts"))
  (gr_line (start 0 6) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 2 3) (size 0.8 0.8) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 10 3) (size 0.8 0.8) (layers "F.Cu") (net 1 "A")))
  (embedded_fonts no)
)"#;

  #[test]
  fn written_copper_is_additive_parses_and_sits_before_embedded_fonts() {
    let original = parse(BOARD).unwrap();
    let mut doc = original.clone();
    let board = Board::read(&doc).unwrap();
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &r, &p, &[]).unwrap();
    let s = route_all(&board, &m, Costs::default());
    let w = add_routes(&mut doc, &m, &board, &s).unwrap();
    assert!(w.segments > 0);
    let text = doc.to_string();
    let back = parse(&text).unwrap();
    assert_eq!(back.to_string(), text);
    let (a, b) = (original.root().unwrap(), back.root().unwrap());
    // Every original item survives, in order, untouched.
    let kept: Vec<_> = b
      .items
      .iter()
      .filter(|n| n.as_list().is_none_or(|l| !l.is("segment") && !l.is("via")))
      .collect();
    assert_eq!(kept.len(), a.items.len());
    assert!(kept.iter().zip(&a.items).all(|(x, y)| *x == y));
    let fonts = b
      .items
      .iter()
      .position(|n| n.as_list().is_some_and(|l| l.is("embedded_fonts")))
      .unwrap();
    let last_seg = b
      .items
      .iter()
      .rposition(|n| n.as_list().is_some_and(|l| l.is("segment")))
      .unwrap();
    assert!(last_seg < fonts);
    // Same routes, same bytes.
    let mut doc2 = original.clone();
    add_routes(&mut doc2, &m, &board, &route_all(&board, &m, Costs::default())).unwrap();
    assert_eq!(doc2.to_string(), text);
  }

  #[test]
  fn uuid_is_deterministic_and_shaped() {
    let u = uuid("x");
    assert_eq!(u, uuid("x"));
    assert_ne!(u, uuid("y"));
    let parts: Vec<&str> = u.split('-').collect();
    assert_eq!(
      parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
      vec![8, 4, 4, 4, 12]
    );
    assert!(parts[2].starts_with('4'));
  }

  #[test]
  fn segment_text_parses_and_matches_kicad_layout() {
    let t = segment(Point::new(1.0, 2.5), Point::new(3.0, 2.5), 0.0889, "F.Cu", 7, "7");
    let d = parse(&t).unwrap();
    let l = d.items.first().and_then(Node::as_list).unwrap();
    assert!(l.is("segment"));
    assert_eq!(l.child("net").and_then(|n| n.number(1)), Some(7.0));
    assert_eq!(l.child("width").and_then(|n| n.number(1)), Some(0.0889));
    assert!(t.starts_with("\n\t(segment\n\t\t(start 1 2.5)\n"));
  }

  /// A KiCad 10 style board (file version 20260206): no top-level net table,
  /// every reference by name. Routes written to it must refer to nets by name
  /// and read back as the same nets.
  const BOARD_BY_NAME: &str = r#"(kicad_pcb
  (version 20260206)
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (gr_line (start 0 0) (end 12 0) (layer "Edge.Cuts"))
  (gr_line (start 12 0) (end 12 6) (layer "Edge.Cuts"))
  (gr_line (start 12 6) (end 0 6) (layer "Edge.Cuts"))
  (gr_line (start 0 6) (end 0 0) (layer "Edge.Cuts"))
  (footprint "X" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 2 3) (size 0.8 0.8) (layers "F.Cu") (net "Sig A"))
    (pad "2" smd rect (at 10 3) (size 0.8 0.8) (layers "F.Cu") (net "Sig A"))
    (pad "3" smd rect (at 2 5) (size 0.8 0.8) (layers "F.Cu"))
  )
  (embedded_fonts no)
)"#;

  #[test]
  fn name_style_board_routes_and_writes_nets_by_name() {
    let mut doc = parse(BOARD_BY_NAME).unwrap();
    let board = Board::read(&doc).unwrap();
    assert!(board.net_table.by_name_style);
    assert_eq!(board.nets.values().collect::<Vec<_>>(), vec!["Sig A"]);
    let mut r = DesignRules::explicit(0.2, 0.2, 0.6, 0.3).unwrap();
    r.min.copper_edge_clearance = 0.3;
    let p = r.classes[0].clone();
    let m = RoutingModel::build(&doc, &board, &r, &p, &[]).unwrap();
    let s = route_all(&board, &m, Costs::default());
    assert_eq!(s.nets_complete, s.nets_total);
    let w = add_routes(&mut doc, &m, &board, &s).unwrap();
    assert!(w.segments > 0);
    let text = doc.to_string();
    assert!(text.contains("(net \"Sig A\")"), "{text}");
    assert!(!text.contains("(net 1)"), "{text}");
    // Reading it back finds the same net on the new copper.
    let back = parse(&text).unwrap();
    let b2 = Board::read(&back).unwrap();
    let root = back.root().unwrap();
    let seg_nets: Vec<Option<u32>> = root
      .children("segment")
      .map(|l| b2.net_table.code_of(l))
      .collect();
    assert!(!seg_nets.is_empty() && seg_nets.iter().all(|n| *n == Some(1)));
    // A second write of the same routes gives the same bytes.
    let mut doc2 = parse(BOARD_BY_NAME).unwrap();
    add_routes(&mut doc2, &m, &board, &s).unwrap();
    assert_eq!(doc2.to_string(), text);
  }
}
