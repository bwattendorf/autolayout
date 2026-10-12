//! What kind of part a footprint is, from the part's own value.
//!
//! "Ask the part": a passive's value carries its unit (`4.7uH`, `100nF`,
//! `10k`), and the unit says what it is. This is a property of the part, not
//! a name the design chose for a net or a refdes it happened to assign. A
//! value that does not parse as a passive leaves the part as `Other` and the
//! grouping treats it by pin count and pin types instead.
//!
//! Formats accepted, as written by KiCad users and in the corpus:
//! - capacitors `100nF`, `4.7uF`, `15pF`, `1µF` (the `F` is required);
//! - inductors `4.7uH`, `10nH`, `1mH` (the `H` is required);
//! - resistors `10k`, `4.7k`, `4k7`, `680R`, `0R`, `1M`, `100Ω`, and
//!   `22R_PP` (a suffix after `_` is a variant tag and ignored).
//!
//! A bare number (`10`, `100n`) is rejected rather than guessed at.

use autolayout_board::Footprint;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
  Resistor,
  Capacitor,
  Inductor,
  /// Anything else: ICs, connectors, diodes, transistors, test points, …
  Other,
}

/// Classify a footprint. Passives must also have exactly two pads with a
/// number, which keeps an IC whose value happens to look like `10k` out.
pub fn kind_of(fp: &Footprint) -> Kind {
  let numbered = fp.pads.iter().filter(|p| !p.number.is_empty()).count();
  if numbered != 2 {
    return Kind::Other;
  }
  kind_of_value(&fp.value)
}

pub fn kind_of_value(value: &str) -> Kind {
  let v = value.split('_').next().unwrap_or_default().trim();
  let digits_end = v
    .find(|c: char| !(c.is_ascii_digit() || c == '.'))
    .unwrap_or(v.len());
  let (num, rest) = v.split_at(digits_end);
  if num.is_empty() || num.parse::<f64>().is_err() {
    return Kind::Other;
  }
  let mut chars = rest.chars();
  let first = chars.next();
  let tail: String = chars.collect();
  let si = |c: char| matches!(c, 'p' | 'n' | 'u' | 'µ' | 'm');
  match (first, tail.as_str()) {
    (Some('F'), "") => Kind::Capacitor,
    (Some(c), "F") if si(c) => Kind::Capacitor,
    (Some('H'), "") => Kind::Inductor,
    (Some(c), "H") if si(c) => Kind::Inductor,
    (Some('R' | 'Ω' | 'k' | 'K' | 'M'), t) if t.is_empty() || t.chars().all(|c| c.is_ascii_digit()) => {
      Kind::Resistor
    }
    _ => Kind::Other,
  }
}

/// Capacitance in farads from a capacitor value (`4.7uF` → 4.7e-6).
pub fn farads(value: &str) -> Option<f64> {
  let v = value.split('_').next()?.trim().strip_suffix('F')?;
  let (num, mult) = match v.chars().last()? {
    'p' => (v.strip_suffix('p')?, 1e-12),
    'n' => (v.strip_suffix('n')?, 1e-9),
    'u' => (v.strip_suffix('u')?, 1e-6),
    'µ' => (v.strip_suffix('µ')?, 1e-6),
    'm' => (v.strip_suffix('m')?, 1e-3),
    _ => (v, 1.0),
  };
  num
    .parse::<f64>()
    .ok()
    .filter(|f| f.is_finite())
    .map(|f| f * mult)
}

#[cfg(test)]
mod tests {
  #[test]
  fn farads_parse() {
    let close = |v: &str, want: f64| super::farads(v).is_some_and(|f| (f - want).abs() <= want * 1e-12);
    assert!(close("100nF", 100e-9));
    assert!(close("4.7uF", 4.7e-6));
    assert!(close("10F", 10.0));
    assert_eq!(super::farads("10k"), None);
  }

  use super::*;

  #[test]
  fn corpus_value_formats() {
    for v in ["100nF", "4.7uF", "15pF", "22uF", "1µF", "10F"] {
      assert_eq!(kind_of_value(v), Kind::Capacitor, "{v}");
    }
    for v in ["4.7uH", "10nH", "1mH"] {
      assert_eq!(kind_of_value(v), Kind::Inductor, "{v}");
    }
    for v in ["10k", "4.7k", "4k7", "680R", "0R", "1M", "22R_PP", "140k", "100Ω"] {
      assert_eq!(kind_of_value(v), Kind::Resistor, "{v}");
    }
    for v in [
      "BAT54S",
      "M4",
      "XTAL_12MHz",
      "RP2354A",
      "SMBJ28CA",
      "",
      "10",
      "TB5.08-6P",
      "USB_C",
    ] {
      assert_eq!(kind_of_value(v), Kind::Other, "{v}");
    }
  }
}
