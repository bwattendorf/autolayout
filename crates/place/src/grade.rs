//! Connection grade of a part (2026-10-16, the owner's idea).
//!
//! A part's **grade** is the number of other parts it shares a signal net with.
//! Nets with more than `plane_fanout` pads are planes or rails a pour serves,
//! not connections, and are left out (as the spring model leaves them out).
//! Nothing here looks at a net name or a reference (CLAUDE.md rule 2): grade and
//! links come from which pads sit on which nets.
//!
//! Reading the grade as a placement order: a high-grade part is a hub (an MCU
//! shares nets with dozens of parts), a low-grade part serves a hub (a bypass
//! cap or a pull-up has one or two neighbours), a mid-grade part joins hubs.
//! Hubs go first and get the room; low-grade parts go last, at the pad of the
//! hub they connect to; mid-grade parts go between the hubs they link. A
//! [`Link`] says which pad of this part meets which pad of the other part, on
//! which net, so a part can be put pad to pad.

use std::collections::{BTreeSet, HashMap};

use autolayout_board::Board;

/// One shared net between two parts, seen from the first part.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
  /// The other part.
  pub other: usize,
  /// Index into this part's `pads`.
  pub my_pad: usize,
  /// Index into the other part's `pads`.
  pub their_pad: usize,
  /// Pads on the net: a two-pad net is a direct wire, a fifteen-pad net a weak tie.
  pub net_pads: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Grades {
  /// Per footprint: how many other parts it shares a signal net with.
  pub grade: Vec<usize>,
  /// Per footprint: its links, in net then pad order (deterministic).
  pub links: Vec<Vec<Link>>,
}

impl Grades {
  /// Weight of a link when pulling a part toward the other part's pad: a
  /// direct wire pulls hard, a net with many pads pulls weakly.
  pub fn weight(link: &Link) -> f64 {
    1.0 / (link.net_pads.saturating_sub(1)).max(1) as f64
  }
}

/// Grades of every footprint of `board`.
pub fn grades(board: &Board, plane_fanout: usize) -> Grades {
  let n = board.footprints.len();
  let mut pads_of: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
  for (f, fp) in board.footprints.iter().enumerate() {
    for (p, pad) in fp.pads.iter().enumerate() {
      if let Some(net) = pad.net.filter(|&n| n != 0) {
        pads_of.entry(net).or_default().push((f, p));
      }
    }
  }
  let mut nets: Vec<u32> = pads_of
    .iter()
    .filter(|(_, v)| (2..=plane_fanout).contains(&v.len()))
    .map(|(&k, _)| k)
    .collect();
  nets.sort_unstable();
  let mut links: Vec<Vec<Link>> = vec![Vec::new(); n];
  for net in nets {
    let Some(pads) = pads_of.get(&net) else { continue };
    for &(f1, p1) in pads {
      for &(f2, p2) in pads {
        if f1 != f2
          && let Some(slot) = links.get_mut(f1)
        {
          slot.push(Link {
            other: f2,
            my_pad: p1,
            their_pad: p2,
            net_pads: pads.len(),
          });
        }
      }
    }
  }
  let grade = links
    .iter()
    .map(|l| l.iter().map(|k| k.other).collect::<BTreeSet<_>>().len())
    .collect();
  Grades { grade, links }
}

#[cfg(test)]
mod tests {
  use super::*;
  use autolayout_sexpr::parse;

  /// U1 joins R1 and R2 on two nets; C1 sits on R1's other net; a ground net
  /// of three pads is a plane at fan-out 2 and must not count.
  const BOARD: &str = r#"(kicad_pcb
  (net 0 "") (net 1 "A") (net 2 "B") (net 3 "C") (net 4 "G")
  (footprint "U" (layer "F.Cu") (at 0 0) (property "Reference" "U1")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net 2 "B"))
    (pad "3" smd rect (at 4 0) (size 1 1) (layers "F.Cu") (net 4 "G")))
  (footprint "R" (layer "F.Cu") (at 10 0) (property "Reference" "R1")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "A"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net 3 "C")))
  (footprint "R" (layer "F.Cu") (at 20 0) (property "Reference" "R2")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 2 "B"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net 4 "G")))
  (footprint "C" (layer "F.Cu") (at 30 0) (property "Reference" "C1")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 3 "C"))
    (pad "2" smd rect (at 2 0) (size 1 1) (layers "F.Cu") (net 4 "G")))
)"#;

  #[test]
  fn grade_counts_other_parts_on_signal_nets_and_ignores_planes() {
    let doc = parse(BOARD).unwrap();
    let b = Board::read(&doc).unwrap();
    // Net G has 3 pads: a plane at fan-out 2.
    let g = grades(&b, 2);
    let by_ref = |r: &str| g.grade[b.footprints.iter().position(|f| f.reference == r).unwrap()];
    assert_eq!(by_ref("U1"), 2, "U1 shares A with R1 and B with R2");
    assert_eq!(by_ref("R1"), 2, "R1 shares A with U1 and C with C1");
    assert_eq!(by_ref("R2"), 1, "R2 shares only B with U1 (G is a plane)");
    assert_eq!(by_ref("C1"), 1, "C1 shares only C with R1");
    // With a generous fan-out the ground net counts and every part joins every other on it.
    let g = grades(&b, 16);
    let by_ref = |r: &str| g.grade[b.footprints.iter().position(|f| f.reference == r).unwrap()];
    assert_eq!(by_ref("C1"), 3);
    // Links name the pads: R2's only link goes to U1's pad 2 from its own pad 1.
    let g = grades(&b, 2);
    let r2 = b.footprints.iter().position(|f| f.reference == "R2").unwrap();
    let u1 = b.footprints.iter().position(|f| f.reference == "U1").unwrap();
    assert_eq!(
      g.links[r2],
      vec![Link {
        other: u1,
        my_pad: 0,
        their_pad: 1,
        net_pads: 2
      }]
    );
    // A direct wire pulls harder than a weak tie.
    assert!(
      Grades::weight(&g.links[r2][0])
        > Grades::weight(&Link {
          net_pads: 9,
          ..g.links[r2][0].clone()
        })
    );
  }
}
