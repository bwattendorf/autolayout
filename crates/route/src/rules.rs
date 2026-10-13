//! Design rules, read from the project file beside the board (M7.0).
//!
//! A router must never guess its rules. The Freerouting harness once routed
//! m1 at KiCad's defaults (0.2 mm track, 0.6/0.3 mm via) because the prepared
//! board was saved without its `.kicad_pro`; the board's real fab rules are
//! 0.0889 mm track with a 0.25/0.15 mm via. So here: no project found and no
//! explicit rules given is an error, and any field we need that is missing or
//! not a finite positive number is an error naming the field.
//!
//! The project file is JSON. The keys read are the ones present in the
//! corpus `.kicad_pro` files (KiCad 9): `net_settings.classes[]`
//! (`name`, `clearance`, `track_width`, `via_diameter`, `via_drill`),
//! `net_settings.netclass_patterns[]` (`netclass`, `pattern`),
//! `net_settings.netclass_assignments` (net name → class, or null) and
//! `board.design_settings.rules.min_*`. Format reference:
//! <https://dev-docs.kicad.org/en/file-formats/> (project file is not
//! specified there; the key names above come from real files, not docs).
//!
//! Assumptions, not verified against KiCad: pattern match supports `*` and
//! `?` wildcards only; an explicit assignment wins over a pattern; the first
//! matching pattern wins; a net that matches neither uses `Default`. A net
//! name here is data the project itself maps to a class — the user's own
//! declaration, not a name the code decides anything from (CLAUDE.md rule 2).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Dimensions of one net class, millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct NetClass {
  pub name: String,
  pub clearance: f64,
  pub track_width: f64,
  pub via_diameter: f64,
  pub via_drill: f64,
}

/// Board-wide minimums from `design_settings.rules`, millimetres.
#[derive(Debug, Clone, PartialEq)]
pub struct Minimums {
  pub clearance: f64,
  pub track_width: f64,
  pub via_diameter: f64,
  pub through_hole_diameter: f64,
  pub copper_edge_clearance: f64,
  pub hole_clearance: f64,
  pub hole_to_hole: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignRules {
  pub classes: Vec<NetClass>,
  /// `(wildcard pattern, class name)`, in file order.
  pub patterns: Vec<(String, String)>,
  /// Explicit net name → class name.
  pub assignments: BTreeMap<String, String>,
  pub min: Minimums,
  /// Where the rules came from, for reports.
  pub source: String,
}

#[derive(Debug, PartialEq)]
pub enum RulesError {
  /// No `.kicad_pro` beside the board and no rules on the command line.
  NoProject(PathBuf),
  Read(String),
  Json(String),
  /// A needed field is absent or not a finite positive number.
  Field(String),
  /// A pattern or assignment names a class the project does not define.
  UnknownClass(String),
  NoDefault,
}

impl fmt::Display for RulesError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      RulesError::NoProject(p) => write!(
        f,
        "no project file at {}; refusing to guess design rules (give --track, --clearance, --via explicitly)",
        p.display()
      ),
      RulesError::Read(e) => write!(f, "{e}"),
      RulesError::Json(e) => write!(f, "project file is not valid JSON: {e}"),
      RulesError::Field(p) => write!(f, "project rules: `{p}` missing or not a finite positive number"),
      RulesError::UnknownClass(c) => write!(f, "project rules: net class `{c}` is used but not defined"),
      RulesError::NoDefault => write!(f, "project rules: no `Default` net class"),
    }
  }
}

impl std::error::Error for RulesError {}

/// The project file that belongs to a board: same stem, `.kicad_pro`.
pub fn project_path(board: &Path) -> PathBuf {
  board.with_extension("kicad_pro")
}

fn positive(v: Option<&Value>, path: &str) -> Result<f64, RulesError> {
  match v.and_then(Value::as_f64) {
    Some(x) if x.is_finite() && x > 0.0 => Ok(x),
    _ => Err(RulesError::Field(path.to_string())),
  }
}

/// Like `positive`, but zero is a legitimate value (a minimum of "none").
fn non_negative(v: Option<&Value>, path: &str) -> Result<f64, RulesError> {
  match v.and_then(Value::as_f64) {
    Some(x) if x.is_finite() && x >= 0.0 => Ok(x),
    _ => Err(RulesError::Field(path.to_string())),
  }
}

impl DesignRules {
  /// Parse a project file's text. `source` labels it in reports.
  pub fn from_json(text: &str, source: &str) -> Result<DesignRules, RulesError> {
    let root: Value = serde_json::from_str(text).map_err(|e| RulesError::Json(e.to_string()))?;
    let ns = root.get("net_settings");

    let mut classes = Vec::new();
    for (i, c) in ns
      .and_then(|n| n.get("classes"))
      .and_then(Value::as_array)
      .map(Vec::as_slice)
      .unwrap_or(&[])
      .iter()
      .enumerate()
    {
      let p = |k: &str| format!("net_settings.classes[{i}].{k}");
      let name = c
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RulesError::Field(p("name")))?;
      classes.push(NetClass {
        name: name.to_string(),
        clearance: non_negative(c.get("clearance"), &p("clearance"))?,
        track_width: positive(c.get("track_width"), &p("track_width"))?,
        via_diameter: positive(c.get("via_diameter"), &p("via_diameter"))?,
        via_drill: positive(c.get("via_drill"), &p("via_drill"))?,
      });
    }
    if !classes.iter().any(|c| c.name == "Default") {
      return Err(RulesError::NoDefault);
    }

    let mut patterns = Vec::new();
    for (i, p) in ns
      .and_then(|n| n.get("netclass_patterns"))
      .and_then(Value::as_array)
      .map(Vec::as_slice)
      .unwrap_or(&[])
      .iter()
      .enumerate()
    {
      let get = |k: &str| {
        p.get(k)
          .and_then(Value::as_str)
          .map(str::to_string)
          .ok_or_else(|| RulesError::Field(format!("net_settings.netclass_patterns[{i}].{k}")))
      };
      patterns.push((get("pattern")?, get("netclass")?));
    }

    let mut assignments = BTreeMap::new();
    if let Some(m) = ns
      .and_then(|n| n.get("netclass_assignments"))
      .and_then(Value::as_object)
    {
      for (net, class) in m {
        let class = class
          .as_str()
          .ok_or_else(|| RulesError::Field(format!("net_settings.netclass_assignments.{net}")))?;
        assignments.insert(net.clone(), class.to_string());
      }
    }

    for c in patterns.iter().map(|(_, c)| c).chain(assignments.values()) {
      if !classes.iter().any(|k| &k.name == c) {
        return Err(RulesError::UnknownClass(c.clone()));
      }
    }

    let r = root
      .get("board")
      .and_then(|b| b.get("design_settings"))
      .and_then(|d| d.get("rules"));
    let m = |k: &str| r.and_then(|r| r.get(k));
    let path = |k: &str| format!("board.design_settings.rules.{k}");
    let min = Minimums {
      clearance: non_negative(m("min_clearance"), &path("min_clearance"))?,
      track_width: positive(m("min_track_width"), &path("min_track_width"))?,
      via_diameter: positive(m("min_via_diameter"), &path("min_via_diameter"))?,
      through_hole_diameter: positive(m("min_through_hole_diameter"), &path("min_through_hole_diameter"))?,
      copper_edge_clearance: non_negative(
        m("min_copper_edge_clearance"),
        &path("min_copper_edge_clearance"),
      )?,
      hole_clearance: non_negative(m("min_hole_clearance"), &path("min_hole_clearance"))?,
      hole_to_hole: non_negative(m("min_hole_to_hole"), &path("min_hole_to_hole"))?,
    };

    Ok(DesignRules {
      classes,
      patterns,
      assignments,
      min,
      source: source.to_string(),
    })
  }

  /// Read the project beside `board`. Missing project is an error.
  pub fn load_for_board(board: &Path) -> Result<DesignRules, RulesError> {
    let pro = project_path(board);
    if !pro.is_file() {
      return Err(RulesError::NoProject(pro));
    }
    let text =
      std::fs::read_to_string(&pro).map_err(|e| RulesError::Read(format!("{}: {e}", pro.display())))?;
    DesignRules::from_json(&text, &pro.display().to_string())
  }

  /// Rules given on the command line instead of a project: one class,
  /// every net in it. Every value must be given; none is defaulted.
  pub fn explicit(
    track: f64,
    clearance: f64,
    via_diameter: f64,
    via_drill: f64,
  ) -> Result<DesignRules, RulesError> {
    let class = NetClass {
      name: "Default".to_string(),
      clearance: non_negative(Some(&Value::from(clearance)), "--clearance")?,
      track_width: positive(Some(&Value::from(track)), "--track")?,
      via_diameter: positive(Some(&Value::from(via_diameter)), "--via")?,
      via_drill: positive(Some(&Value::from(via_drill)), "--via")?,
    };
    if class.via_drill >= class.via_diameter {
      return Err(RulesError::Field(
        "--via (drill must be smaller than diameter)".to_string(),
      ));
    }
    // Explicit mode has no project to say otherwise, so the hole and edge
    // minimums are set equal to the clearance: stated here, not measured.
    let min = Minimums {
      clearance: class.clearance,
      track_width: class.track_width,
      via_diameter: class.via_diameter,
      through_hole_diameter: class.via_drill,
      copper_edge_clearance: class.clearance,
      hole_clearance: class.clearance,
      hole_to_hole: class.clearance,
    };
    Ok(DesignRules {
      classes: vec![class],
      patterns: Vec::new(),
      assignments: BTreeMap::new(),
      min,
      source: "command line".to_string(),
    })
  }

  fn class_named(&self, name: &str) -> Option<&NetClass> {
    self.classes.iter().find(|c| c.name == name)
  }

  /// The class a net routes under: explicit assignment, then first matching
  /// pattern, then `Default`.
  pub fn class_for_net(&self, net: &str) -> &NetClass {
    let named = self.assignments.get(net).map(String::as_str).or_else(|| {
      self
        .patterns
        .iter()
        .find(|(p, _)| wildcard_match(p, net))
        .map(|(_, c)| c.as_str())
    });
    // `Default` was checked present at load; classes[0] only guards a
    // hand-built value with no classes, which then has no rules to give.
    named
      .and_then(|n| self.class_named(n))
      .or_else(|| self.class_named("Default"))
      .unwrap_or(&self.classes[0])
  }

  /// Clearance between two nets' copper: the larger of the two classes'
  /// clearances and the board minimum.
  pub fn clearance_between(&self, a: &str, b: &str) -> f64 {
    self
      .class_for_net(a)
      .clearance
      .max(self.class_for_net(b).clearance)
      .max(self.min.clearance)
  }

  /// Report, one line per class plus the minimums.
  pub fn report(&self) -> String {
    let mut s = format!("rules from {}\n", self.source);
    for c in &self.classes {
      s += &format!(
        "  class {:<10} track {:.4}  clearance {:.4}  via {:.4}/{:.4}\n",
        c.name, c.track_width, c.clearance, c.via_diameter, c.via_drill
      );
    }
    s += &format!(
      "  patterns {}  assignments {}\n  min: clearance {:.4}  track {:.4}  via {:.4}  hole {:.4}  edge {:.4}  hole-clearance {:.4}  hole-to-hole {:.4}\n",
      self.patterns.len(),
      self.assignments.len(),
      self.min.clearance,
      self.min.track_width,
      self.min.via_diameter,
      self.min.through_hole_diameter,
      self.min.copper_edge_clearance,
      self.min.hole_clearance,
      self.min.hole_to_hole
    );
    s
  }
}

/// `*` matches any run (including empty), `?` any one character.
fn wildcard_match(pattern: &str, text: &str) -> bool {
  let p: Vec<char> = pattern.chars().collect();
  let t: Vec<char> = text.chars().collect();
  let (mut pi, mut ti) = (0, 0);
  let (mut star, mut mark) = (None, 0);
  while ti < t.len() {
    match p.get(pi) {
      Some('*') => {
        star = Some(pi);
        mark = ti;
        pi += 1;
      }
      Some(&c) if c == '?' || c == t[ti] => {
        pi += 1;
        ti += 1;
      }
      _ => match star {
        Some(s) => {
          pi = s + 1;
          mark += 1;
          ti = mark;
        }
        None => return false,
      },
    }
  }
  while p.get(pi) == Some(&'*') {
    pi += 1;
  }
  pi == p.len()
}

#[cfg(test)]
mod tests {
  use super::*;

  const PRO: &str = r#"{
    "board": {"design_settings": {"rules": {
      "min_clearance": 0.0889, "min_track_width": 0.0889, "min_via_diameter": 0.25,
      "min_through_hole_diameter": 0.15, "min_copper_edge_clearance": 0.3,
      "min_hole_clearance": 0.0889, "min_hole_to_hole": 0.0889}}},
    "net_settings": {
      "classes": [
        {"name": "Default", "clearance": 0.0889, "track_width": 0.0889, "via_diameter": 0.25, "via_drill": 0.15},
        {"name": "Power", "clearance": 0.2, "track_width": 0.5, "via_diameter": 0.8, "via_drill": 0.4}],
      "netclass_patterns": [{"netclass": "Power", "pattern": "+12V*"}],
      "netclass_assignments": {"VBAT": "Power"}
    }}"#;

  #[test]
  fn reads_classes_and_minimums() {
    let r = DesignRules::from_json(PRO, "test").unwrap();
    assert_eq!(r.classes.len(), 2);
    assert_eq!(r.class_for_net("SIG").track_width, 0.0889);
    assert_eq!(r.min.via_diameter, 0.25);
  }

  #[test]
  fn pattern_and_assignment_pick_the_class() {
    let r = DesignRules::from_json(PRO, "test").unwrap();
    assert_eq!(r.class_for_net("+12V_IN").name, "Power");
    assert_eq!(r.class_for_net("VBAT").name, "Power");
    assert_eq!(r.class_for_net("+12").name, "Default");
  }

  #[test]
  fn clearance_is_the_larger_class() {
    let r = DesignRules::from_json(PRO, "test").unwrap();
    assert_eq!(r.clearance_between("SIG", "SIG2"), 0.0889);
    assert_eq!(r.clearance_between("SIG", "VBAT"), 0.2);
  }

  #[test]
  fn missing_field_is_an_error_naming_it() {
    let bad = PRO.replace("\"via_drill\": 0.15", "\"via_x\": 1");
    let e = DesignRules::from_json(&bad, "t").unwrap_err();
    assert_eq!(e, RulesError::Field("net_settings.classes[0].via_drill".into()));
  }

  #[test]
  fn non_finite_or_zero_width_is_refused() {
    let bad = PRO.replace("\"track_width\": 0.5", "\"track_width\": 0");
    assert!(matches!(
      DesignRules::from_json(&bad, "t"),
      Err(RulesError::Field(_))
    ));
  }

  #[test]
  fn unknown_class_and_no_default_are_errors() {
    let bad = PRO.replace("\"VBAT\": \"Power\"", "\"VBAT\": \"Nope\"");
    assert_eq!(
      DesignRules::from_json(&bad, "t").unwrap_err(),
      RulesError::UnknownClass("Nope".into())
    );
    let bad = PRO.replace("\"Default\"", "\"Other\"");
    assert_eq!(
      DesignRules::from_json(&bad, "t").unwrap_err(),
      RulesError::NoDefault
    );
  }

  #[test]
  fn garbage_is_an_error_not_a_panic() {
    for t in ["", "{", "[]", "null", "{\"net_settings\": 3}"] {
      assert!(DesignRules::from_json(t, "t").is_err());
    }
  }

  #[test]
  fn no_project_is_refused() {
    let e = DesignRules::load_for_board(Path::new("/nonexistent/dir/x.kicad_pcb")).unwrap_err();
    assert!(matches!(e, RulesError::NoProject(_)));
  }

  #[test]
  fn explicit_rules_need_every_value() {
    assert!(DesignRules::explicit(0.1, 0.1, 0.4, 0.2).is_ok());
    assert!(DesignRules::explicit(0.0, 0.1, 0.4, 0.2).is_err());
    assert!(DesignRules::explicit(0.1, f64::NAN, 0.4, 0.2).is_err());
    assert!(DesignRules::explicit(0.1, 0.1, 0.2, 0.4).is_err());
  }

  #[test]
  fn wildcards() {
    assert!(wildcard_match("*", ""));
    assert!(wildcard_match("a*c", "abbbc"));
    assert!(wildcard_match("a?c", "abc"));
    assert!(!wildcard_match("a?c", "ac"));
    assert!(!wildcard_match("+12V", "+12V_IN"));
  }
}
