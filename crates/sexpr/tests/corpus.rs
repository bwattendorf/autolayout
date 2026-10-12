//! Byte-exact round-trip of every `.kicad_pcb` under `AUTOLAYOUT_CORPUS`.
//!
//! This is the M0 exit condition (ROADMAP). The test skips, saying so, when the
//! variable is unset, so the repo still builds and tests without the corpus.

use std::fs;
use std::path::{Path, PathBuf};

use autolayout_sexpr::parse;

#[test]
fn every_corpus_board_round_trips() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let mut boards = Vec::new();
  collect(Path::new(&root), &mut boards);
  assert!(!boards.is_empty(), "no .kicad_pcb files under {root}");

  let mut failures = Vec::new();
  for path in &boards {
    let src = fs::read_to_string(path).unwrap();
    match parse(&src) {
      Ok(doc) if doc.to_string() == src => {}
      Ok(_) => failures.push(format!("{}: parsed but output differs", path.display())),
      Err(e) => failures.push(format!("{}: {e}", path.display())),
    }
  }
  eprintln!(
    "{} of {} corpus boards round-trip byte-exact",
    boards.len() - failures.len(),
    boards.len()
  );
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
  let Ok(entries) = fs::read_dir(dir) else { return };
  for entry in entries.flatten() {
    let path = entry.path();
    if path.is_dir() {
      collect(&path, out);
    } else if path.extension().is_some_and(|e| e == "kicad_pcb") {
      out.push(path);
    }
  }
}
