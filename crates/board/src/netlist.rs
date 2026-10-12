//! KiCad netlist (`.net`, the `(export …)` s-expression) — the source of pin
//! electrical types, which script-imported boards do not carry.
//!
//! Shape relied on (KiCad 9 netlist exporter): `(export (nets (net (code "1")
//! (name "…") (node (ref "C22") (pin "1") (pintype "PASSIVE")) …)))`.

use std::collections::HashMap;

use autolayout_sexpr::Document;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Netlist {
  /// (reference, pin number) → electrical type as written (`PASSIVE`,
  /// `POWER-IN`, `BIDIRECTIONAL`, …).
  pub pintype: HashMap<(String, String), String>,
  /// (reference, pin number) → net name.
  pub net_of: HashMap<(String, String), String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetlistError {
  NotANetlist,
}

impl std::fmt::Display for NetlistError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("top-level list is not (export …) or has no (nets …)")
  }
}

impl std::error::Error for NetlistError {}

impl Netlist {
  pub fn read(doc: &Document) -> Result<Netlist, NetlistError> {
    let nets = doc
      .root()
      .filter(|r| r.is("export"))
      .and_then(|r| r.child("nets"))
      .ok_or(NetlistError::NotANetlist)?;
    let mut out = Netlist::default();
    for net in nets.children("net") {
      let name = net.child("name").and_then(|n| n.atom(1)).unwrap_or_default();
      for node in net.children("node") {
        let field = |k: &str| node.child(k).and_then(|f| f.atom(1)).map(|v| v.into_owned());
        let (Some(reference), Some(pin)) = (field("ref"), field("pin")) else {
          continue;
        };
        let key = (reference, pin);
        if let Some(t) = field("pintype") {
          out.pintype.insert(key.clone(), t);
        }
        out.net_of.insert(key, name.clone().into_owned());
      }
    }
    Ok(out)
  }

  pub fn pintype(&self, reference: &str, pin: &str) -> Option<&str> {
    self
      .pintype
      .get(&(reference.to_owned(), pin.to_owned()))
      .map(String::as_str)
  }
}
