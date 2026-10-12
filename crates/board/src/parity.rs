//! Board ↔ netlist parity: does a layout still describe the circuit?
//!
//! Three separate questions, kept separate because their consequences are:
//! - **Parts:** every reference on both sides, with the same value and
//!   footprint. A value mismatch means the BOM and the board disagree.
//! - **Wiring:** which pads are joined. Compared as *sets of (reference,
//!   pad)*, so it does not care what a net is called. A difference here means
//!   the copper connects something the schematic does not, or misses
//!   something it does.
//! - **Names:** nets with identical members but different names. Cosmetic
//!   for fabrication; reported so a rename is never mistaken for a rewire.
//!
//! Single-member nets (an unconnected pin's own net) carry no connectivity and
//! are skipped on both sides.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Board, Netlist};

type Member = (String, String);

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parity {
  pub only_on_board: Vec<String>,
  pub only_in_netlist: Vec<String>,
  /// (reference, board value, netlist value)
  pub value_differs: Vec<(String, String, String)>,
  /// (reference, board footprint, netlist footprint)
  pub footprint_differs: Vec<(String, String, String)>,
  /// Board nets with no netlist net of identical membership:
  /// (board net name, closest netlist net name, pads only on the board side,
  /// pads only on the netlist side).
  pub wiring_differs: Vec<(String, String, Vec<Member>, Vec<Member>)>,
  /// Identical membership, different name: (board name, netlist name).
  pub renamed: Vec<(String, String)>,
  /// Multi-member nets compared.
  pub nets_compared: usize,
}

impl Parity {
  pub fn is_clean_for_fab(&self) -> bool {
    self.only_on_board.is_empty()
      && self.only_in_netlist.is_empty()
      && self.value_differs.is_empty()
      && self.footprint_differs.is_empty()
      && self.wiring_differs.is_empty()
  }
}

pub fn parity(board: &Board, netlist: &Netlist) -> Parity {
  let mut p = Parity::default();

  let board_refs: BTreeMap<&str, (&str, &str)> = board
    .footprints
    .iter()
    .map(|f| (f.reference.as_str(), (f.value.as_str(), f.lib_id.as_str())))
    .collect();
  for (r, (bv, bf)) in &board_refs {
    match netlist.components.get(*r) {
      None => p.only_on_board.push((*r).to_owned()),
      Some((nv, nf)) => {
        if bv != nv {
          p.value_differs
            .push(((*r).to_owned(), (*bv).to_owned(), nv.clone()));
        }
        if bf != nf {
          p.footprint_differs
            .push(((*r).to_owned(), (*bf).to_owned(), nf.clone()));
        }
      }
    }
  }
  p.only_in_netlist = netlist
    .components
    .keys()
    .filter(|r| !board_refs.contains_key(r.as_str()))
    .cloned()
    .collect();
  p.only_in_netlist.sort();

  // Board: net code -> members. Netlist: net name -> members.
  let mut board_nets: BTreeMap<u32, BTreeSet<Member>> = BTreeMap::new();
  for fp in &board.footprints {
    for pad in fp.pads.iter().filter(|p| !p.number.is_empty()) {
      if let Some(n) = pad.net {
        board_nets
          .entry(n)
          .or_default()
          .insert((fp.reference.clone(), pad.number.clone()));
      }
    }
  }
  let mut net_nets: BTreeMap<&str, BTreeSet<Member>> = BTreeMap::new();
  for (member, name) in &netlist.net_of {
    net_nets.entry(name.as_str()).or_default().insert(member.clone());
  }
  net_nets.retain(|_, m| m.len() > 1);
  let by_members: BTreeMap<&BTreeSet<Member>, &str> = net_nets.iter().map(|(n, m)| (m, *n)).collect();

  for (code, members) in board_nets.iter().filter(|(_, m)| m.len() > 1) {
    p.nets_compared += 1;
    let board_name = board
      .nets
      .get(code)
      .cloned()
      .unwrap_or_else(|| format!("net {code}"));
    if let Some(&name) = by_members.get(members) {
      if name != board_name {
        p.renamed.push((board_name, name.to_owned()));
      }
      continue;
    }
    // Closest netlist net: the one sharing the most members.
    let best = net_nets
      .iter()
      .max_by_key(|(_, m)| m.intersection(members).count());
    let (name, theirs) = best.map_or(("(none)", BTreeSet::new()), |(n, m)| (*n, m.clone()));
    p.wiring_differs.push((
      board_name,
      name.to_owned(),
      members.difference(&theirs).cloned().collect(),
      theirs.difference(members).cloned().collect(),
    ));
  }
  p
}

impl std::fmt::Display for Parity {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let verdict = if self.is_clean_for_fab() {
      "PARTS AND WIRING MATCH"
    } else {
      "MISMATCH"
    };
    writeln!(f, "{verdict}  ({} multi-pad nets compared)", self.nets_compared)?;
    writeln!(f, "  parts only on board:      {:?}", self.only_on_board)?;
    writeln!(f, "  parts only in netlist:    {:?}", self.only_in_netlist)?;
    writeln!(f, "  value differs:            {}", self.value_differs.len())?;
    for (r, b, n) in &self.value_differs {
      writeln!(f, "      {r}: board {b:?}  netlist {n:?}")?;
    }
    writeln!(f, "  footprint differs:        {}", self.footprint_differs.len())?;
    for (r, b, n) in &self.footprint_differs {
      writeln!(f, "      {r}: board {b:?}  netlist {n:?}")?;
    }
    writeln!(f, "  wiring differs:           {}", self.wiring_differs.len())?;
    for (b, n, ours, theirs) in &self.wiring_differs {
      writeln!(
        f,
        "      board {b:?} vs netlist {n:?}: board-only {ours:?}  netlist-only {theirs:?}"
      )?;
    }
    write!(f, "  renamed only (cosmetic):  {}", self.renamed.len())?;
    for (b, n) in &self.renamed {
      write!(f, "\n      {b:?} -> {n:?}")?;
    }
    Ok(())
  }
}
