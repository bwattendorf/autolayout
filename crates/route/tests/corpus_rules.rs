//! Every corpus board that has a project file beside it must yield rules, or
//! a named error; none may panic. Skips (printing why) without the corpus.

use std::path::{Path, PathBuf};

use autolayout_route::rules::DesignRules;

fn boards(dir: &Path, out: &mut Vec<PathBuf>) {
  let Ok(rd) = std::fs::read_dir(dir) else { return };
  let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
  entries.sort();
  for p in entries {
    if p.is_dir() {
      boards(&p, out);
    } else if p.extension().is_some_and(|e| e == "kicad_pcb") {
      out.push(p);
    }
  }
}

#[test]
fn corpus_projects_give_rules() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    println!("SKIPPED: route rules corpus test (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let mut all = Vec::new();
  boards(Path::new(&root), &mut all);
  let (mut ok, mut missing) = (0, 0);
  for b in &all {
    match DesignRules::load_for_board(b) {
      Ok(_) => ok += 1,
      Err(autolayout_route::rules::RulesError::NoProject(_)) => missing += 1,
      Err(e) => panic!("{}: {e}", b.display()),
    }
  }
  println!(
    "route rules: {ok} boards with rules, {missing} without a project file, of {}",
    all.len()
  );
  assert!(ok > 0, "no board yielded rules; refusing a vacuous pass");
}

/// m1's real fab rules, the numbers Portcullis recorded (3.5 mil, 0.25/0.15 via).
#[test]
fn m1_fab_rules_are_the_fab_numbers() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    println!("SKIPPED: m1 fab rules (AUTOLAYOUT_CORPUS not set)");
    return;
  };
  let b = Path::new(&root).join("quilter/fab/m1/m1.kicad_pcb");
  let r = DesignRules::load_for_board(&b).unwrap();
  let d = &r.classes[0];
  assert_eq!((d.track_width, d.via_diameter, d.via_drill), (0.0889, 0.25, 0.15));
}
