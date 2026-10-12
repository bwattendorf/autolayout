//! Which parts belong together (M3.1: supplies, MCUs, switching converters).
//!
//! Every decision comes from pin electrical types (netlist), part kinds (the
//! part's own value, [`crate::kind`]) and connectivity. No net name and no
//! reference designator is ever compared to a literal (CLAUDE.md rule 2).
//!
//! Rules, each with the reason it is shaped the way it is:
//! - **Supply net:** any net carrying a `POWER-IN` pin. Not `POWER-OUT`: part
//!   libraries also type antenna drivers and a regulator's switch pin
//!   `POWER-OUT` (Portcullis r board, 2026-10-12), which would make an RF or
//!   switch node a "rail".
//! - **Ground:** the supply net with the most pads. Structural; correct on
//!   every corpus board checked so far, and printed so a wrong pick is seen.
//! - **MCU:** a part with at least 16 `BIDIRECTIONAL` pins. Across the
//!   Portcullis family every RP2350/RP2354 has 20–57 and no other part more
//!   than 9 (measured by the place_floorplan step-1 run, 2026-10-12). A
//!   module that declares fewer (g2n's ESP32-C6, 5) is missed — known.
//! - **Converter:** found from its inductor, not from the regulator's pin
//!   types, because one design can declare the same regulator twice with
//!   different types (Portcullis m1: U3's SW is `POWER-OUT`, U4's is
//!   `PASSIVE`). An inductor is a two-pad part whose value is in henries; its
//!   *switch node* is the end on a non-supply net that also reaches an IC;
//!   its *output* is the other end, which must be a supply net. Then:
//!   - boot cap: a capacitor from the switch node to a net only the regulator
//!     touches;
//!   - feedback: resistors on a non-supply net that holds only resistors and
//!     one regulator pin, where one resistor's far end is the output — the
//!     output-sense condition is what separates FB from an EN pull-up;
//!   - output caps: ≥ 1 µF capacitors from the output to ground; input caps:
//!     ≥ 1 µF capacitors from the regulator's own non-ground supply input to
//!     ground. The 1 µF floor keeps the 100 nF decoupling of every other IC
//!     on the same rail out; a bulk cap on the rail far from the converter
//!     is still counted (known over-reach, placement will show it).
//!   - when the "regulator" is the MCU (an on-chip core buck), input caps are
//!     not determinable structurally: every MCU supply pin is a candidate.
//!     Reported as unknown rather than guessed.

use std::collections::{BTreeSet, HashMap};

use autolayout_board::{Board, Netlist};

use crate::kind::{Kind, farads, kind_of};

/// Capacitors at or above this are bulk (converter input/output), below it
/// decoupling. See the module notes.
const BULK_FARADS: f64 = 1e-6;

/// Pads, by (footprint index, pad index).
type PadRef = (usize, usize);

pub struct Grouping {
  pub supply_nets: BTreeSet<u32>,
  pub ground: Option<u32>,
  pub mcus: Vec<usize>,
  pub fixed: Vec<usize>,
  pub converters: Vec<Converter>,
  /// Pads with no netlist entry, i.e. the join failed for them.
  pub unjoined_pads: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Converter {
  pub regulator: usize,
  /// The regulator is the MCU (on-chip core converter).
  pub on_mcu: bool,
  pub inductor: usize,
  pub switch_net: u32,
  pub output_net: u32,
  pub boot: Vec<usize>,
  pub feedback: Vec<usize>,
  pub output_caps: Vec<usize>,
  /// `None` when not structurally determinable (MCU-hosted converters).
  pub input_caps: Option<Vec<usize>>,
}

struct Ctx<'a> {
  board: &'a Board,
  kinds: Vec<Kind>,
  pintype: HashMap<PadRef, &'a str>,
  net_pads: HashMap<u32, Vec<PadRef>>,
}

impl Ctx<'_> {
  fn net_of(&self, (f, p): PadRef) -> Option<u32> {
    self.board.footprints.get(f)?.pads.get(p)?.net
  }

  fn pads_on(&self, net: u32) -> &[PadRef] {
    self.net_pads.get(&net).map_or(&[], Vec::as_slice)
  }

  fn is_ic(&self, f: usize) -> bool {
    self.kinds.get(f) == Some(&Kind::Other)
      && self
        .board
        .footprints
        .get(f)
        .is_some_and(|fp| fp.pads.iter().filter(|p| !p.number.is_empty()).count() >= 3)
  }

  /// Nets of a two-pad part's two pads, if both are connected.
  fn two_nets(&self, f: usize) -> Option<(u32, u32)> {
    let fp = self.board.footprints.get(f)?;
    let mut nets = fp
      .pads
      .iter()
      .filter(|p| !p.number.is_empty())
      .filter_map(|p| p.net);
    Some((nets.next()?, nets.next()?))
  }

  fn farads(&self, f: usize) -> f64 {
    self
      .board
      .footprints
      .get(f)
      .and_then(|fp| farads(&fp.value))
      .unwrap_or(0.0)
  }

  /// Two-pad parts of `kind` with one pad on `a` and the other on `b`.
  fn bridging(&self, kind: Kind, a: u32, b: u32) -> Vec<usize> {
    let mut out: Vec<usize> = (0..self.board.footprints.len())
      .filter(|&f| self.kinds.get(f) == Some(&kind))
      .filter(|&f| matches!(self.two_nets(f), Some((x, y)) if (x == a && y == b) || (x == b && y == a)))
      .collect();
    out.sort_unstable();
    out
  }
}

pub fn group(board: &Board, netlist: &Netlist) -> Grouping {
  let kinds: Vec<Kind> = board.footprints.iter().map(kind_of).collect();
  let mut pintype = HashMap::new();
  let mut net_pads: HashMap<u32, Vec<PadRef>> = HashMap::new();
  let mut unjoined_pads = 0;
  for (f, fp) in board.footprints.iter().enumerate() {
    for (p, pad) in fp.pads.iter().enumerate() {
      let Some(net) = pad.net else { continue };
      net_pads.entry(net).or_default().push((f, p));
      match netlist.pintype(&fp.reference, &pad.number) {
        Some(t) => {
          pintype.insert((f, p), t);
        }
        None => unjoined_pads += 1,
      }
    }
  }
  let ctx = Ctx {
    board,
    kinds,
    pintype,
    net_pads,
  };

  let supply_nets: BTreeSet<u32> = ctx
    .pintype
    .iter()
    .filter(|(_, t)| **t == "POWER-IN")
    .filter_map(|(&pr, _)| ctx.net_of(pr))
    .collect();
  let ground = supply_nets
    .iter()
    .copied()
    .max_by_key(|&n| (ctx.pads_on(n).len(), std::cmp::Reverse(n)));

  let mcus: Vec<usize> = (0..board.footprints.len())
    .filter(|&f| {
      ctx
        .pintype
        .iter()
        .filter(|((pf, _), t)| *pf == f && **t == "BIDIRECTIONAL")
        .count()
        >= 16
    })
    .collect();
  let fixed: Vec<usize> = board
    .footprints
    .iter()
    .enumerate()
    .filter(|(_, fp)| fp.locked)
    .map(|(f, _)| f)
    .collect();

  let converters = (0..board.footprints.len())
    .filter(|&f| ctx.kinds.get(f) == Some(&Kind::Inductor))
    .filter_map(|l| converter_at(&ctx, l, &supply_nets, ground, &mcus))
    .collect();

  Grouping {
    supply_nets,
    ground,
    mcus,
    fixed,
    converters,
    unjoined_pads,
  }
}

fn converter_at(
  ctx: &Ctx,
  inductor: usize,
  supply: &BTreeSet<u32>,
  ground: Option<u32>,
  mcus: &[usize],
) -> Option<Converter> {
  let (a, b) = ctx.two_nets(inductor)?;
  let reaches_ic = |n: u32| ctx.pads_on(n).iter().any(|&(f, _)| f != inductor && ctx.is_ic(f));
  // A rail is a supply net, or any net with bulk capacitance to ground. The
  // second half matters: a rail that only feeds a connector (Portcullis m1's
  // reader supply) has no POWER-IN pin on it, yet is plainly a converter
  // output. A switch node never carries bulk capacitance to ground.
  let is_rail = |n: u32| {
    supply.contains(&n)
      || ground.is_some_and(|g| {
        ctx
          .bridging(Kind::Capacitor, n, g)
          .iter()
          .any(|&c| ctx.farads(c) >= BULK_FARADS)
      })
  };
  let (switch_net, output_net) = match (is_rail(a), is_rail(b)) {
    (false, true) if reaches_ic(a) => (a, b),
    (true, false) if reaches_ic(b) => (b, a),
    _ => return None, // both rails (a filter), or no IC on the free end
  };
  let regulators: BTreeSet<usize> = ctx
    .pads_on(switch_net)
    .iter()
    .map(|&(f, _)| f)
    .filter(|&f| ctx.is_ic(f))
    .collect();
  let regulator = *regulators.iter().next()?;
  let on_mcu = mcus.contains(&regulator);

  // A net only `regulator` touches, apart from the parts in `except`.
  let private_to = |net: u32, except: usize| {
    ctx
      .pads_on(net)
      .iter()
      .all(|&(f, _)| f == regulator || f == except)
  };

  let boot: Vec<usize> = (0..ctx.board.footprints.len())
    .filter(|&f| ctx.kinds.get(f) == Some(&Kind::Capacitor))
    .filter(|&f| match ctx.two_nets(f) {
      Some((x, y)) if x == switch_net => !supply.contains(&y) && private_to(y, f),
      Some((x, y)) if y == switch_net => !supply.contains(&x) && private_to(x, f),
      _ => false,
    })
    .collect();

  let mut feedback = BTreeSet::new();
  for (&net, pads) in &ctx.net_pads {
    if supply.contains(&net) || net == switch_net {
      continue;
    }
    let reg_pins = pads.iter().filter(|&&(f, _)| f == regulator).count();
    let all_resistors_else = pads
      .iter()
      .all(|&(f, _)| f == regulator || ctx.kinds.get(f) == Some(&Kind::Resistor));
    if reg_pins != 1 || !all_resistors_else {
      continue;
    }
    let resistors: Vec<usize> = pads.iter().map(|&(f, _)| f).filter(|&f| f != regulator).collect();
    let senses_output = resistors
      .iter()
      .any(|&r| matches!(ctx.two_nets(r), Some((x, y)) if x == output_net || y == output_net));
    if senses_output {
      feedback.extend(resistors);
    }
  }

  let gnd = ground?;
  let output_caps: Vec<usize> = ctx
    .bridging(Kind::Capacitor, output_net, gnd)
    .into_iter()
    .filter(|&c| ctx.farads(c) >= BULK_FARADS)
    .collect();
  let input_caps = if on_mcu {
    None
  } else {
    let reg_supplies: BTreeSet<u32> = ctx
      .pintype
      .iter()
      .filter(|((f, _), t)| *f == regulator && **t == "POWER-IN")
      .filter_map(|(&pr, _)| ctx.net_of(pr))
      .filter(|&n| n != gnd && n != output_net)
      .collect();
    let mut caps: Vec<usize> = reg_supplies
      .iter()
      .flat_map(|&n| ctx.bridging(Kind::Capacitor, n, gnd))
      .filter(|&c| ctx.farads(c) >= BULK_FARADS)
      .collect();
    caps.sort_unstable();
    caps.dedup();
    Some(caps)
  };

  Some(Converter {
    regulator,
    on_mcu,
    inductor,
    switch_net,
    output_net,
    boot,
    feedback: feedback.into_iter().collect(),
    output_caps,
    input_caps,
  })
}

impl Grouping {
  /// Human-readable report; references and net names are printed for the
  /// reader, never used to decide anything.
  pub fn report(&self, board: &Board) -> String {
    let r = |f: usize| {
      board
        .footprints
        .get(f)
        .map_or("?", |fp| fp.reference.as_str())
        .to_owned()
    };
    let refs = |v: &[usize]| v.iter().map(|&f| r(f)).collect::<Vec<_>>().join(" ");
    let net = |n: u32| board.nets.get(&n).cloned().unwrap_or_else(|| format!("net {n}"));
    let mut s = String::new();
    let supplies: Vec<String> = self.supply_nets.iter().map(|&n| net(n)).collect();
    s += &format!("supply nets ({}): {}\n", supplies.len(), supplies.join(" "));
    s += &format!("ground: {}\n", self.ground.map_or("none found".to_owned(), net));
    s += &format!(
      "MCUs: {}\n",
      if self.mcus.is_empty() {
        "none".to_owned()
      } else {
        refs(&self.mcus)
      }
    );
    s += &format!("fixed (locked): {}\n", self.fixed.len());
    s += &format!("converters: {}\n", self.converters.len());
    for c in &self.converters {
      s += &format!(
        "  {}{}  L {}  switch {} -> {}\n    boot [{}]  feedback [{}]  out caps [{}]  in caps [{}]\n",
        r(c.regulator),
        if c.on_mcu { " (MCU core)" } else { "" },
        r(c.inductor),
        net(c.switch_net),
        net(c.output_net),
        refs(&c.boot),
        refs(&c.feedback),
        refs(&c.output_caps),
        c.input_caps
          .as_deref()
          .map_or("not determinable structurally".to_owned(), refs),
      );
    }
    if self.unjoined_pads > 0 {
      s += &format!(
        "WARNING: {} netted pads have no netlist entry\n",
        self.unjoined_pads
      );
    }
    s
  }
}
