//! The grader's courtyard overlaps against KiCad's own DRC, board by board.
//!
//! For every placed board in the corpus (`quilter/returned/**` and
//! `quilter/fab/**`), the set of overlapping pairs `autolayout grade` reports
//! must equal the set of `[courtyards_overlap]` pairs `kicad-cli pcb drc`
//! reports. Exact agreement, both directions: a pair only we report is a false
//! alarm (the R6/C22 class); a pair only KiCad reports is a miss (the ganged
//! terminal-block class). Skips, saying why, without the corpus or KiCad.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use autolayout_board::Board;
use autolayout_sexpr::parse;

const KICAD_CLI: &str = "C:/Program Files/KiCad/9.0/bin/kicad-cli.exe";

/// Slow: a full KiCad DRC per board, ~20 min for the 35-board corpus
/// (measured 2026-10-12). Run with `./ci.sh --kicad-full`, and always after
/// changing courtyard geometry or the overlap test.
#[test]
#[ignore = "slow: full KiCad DRC per board; run via ./ci.sh --kicad-full"]
fn courtyard_overlaps_match_kicad_drc() {
  let Ok(root) = std::env::var("AUTOLAYOUT_CORPUS") else {
    eprintln!("SKIPPED: AUTOLAYOUT_CORPUS is not set");
    return;
  };
  let cli = std::env::var("KICAD_CLI").unwrap_or_else(|_| KICAD_CLI.to_owned());
  if !Path::new(&cli).exists() {
    eprintln!("SKIPPED: no kicad-cli at {cli}");
    return;
  }
  let mut boards = Vec::new();
  for sub in ["quilter/returned", "quilter/fab"] {
    collect(&Path::new(&root).join(sub), &mut boards);
  }
  boards.sort();
  assert!(boards.len() >= 10, "too few placed boards found under {root}");

  let tmp = std::env::temp_dir().join(format!("autolayout-drc-{}", std::process::id()));
  fs::create_dir_all(&tmp).unwrap();
  let (mut agree, mut kicad_pairs, mut failures) = (0, 0, Vec::new());
  for (i, path) in boards.iter().enumerate() {
    let report = tmp.join(format!("{i}.rpt"));
    let status = Command::new(&cli)
      .args(["pcb", "drc", "-o"])
      .arg(&report)
      .arg(path)
      .output()
      .unwrap();
    assert!(
      status.status.success() || report.exists(),
      "kicad-cli failed on {}",
      path.display()
    );
    let theirs = kicad_overlaps(&fs::read_to_string(&report).unwrap());
    let doc = parse(&fs::read_to_string(path).unwrap()).unwrap();
    let ours: BTreeSet<(String, String)> = autolayout_grade::grade(&Board::read(&doc).unwrap())
      .overlaps
      .into_iter()
      .collect();
    kicad_pairs += theirs.len();
    if ours == theirs {
      agree += 1;
    } else {
      let only_ours: Vec<_> = ours.difference(&theirs).collect();
      let only_kicad: Vec<_> = theirs.difference(&ours).collect();
      failures.push(format!(
        "{}\n    only ours: {only_ours:?}\n    only KiCad: {only_kicad:?}",
        path.display()
      ));
    }
  }
  let _ = fs::remove_dir_all(&tmp); // best effort; a leftover temp dir is harmless
  eprintln!(
    "courtyard overlaps: {agree} of {} boards agree exactly with KiCad DRC ({kicad_pairs} KiCad pairs in total)",
    boards.len()
  );
  assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Pairs from KiCad's text report:
/// `[courtyards_overlap]: …` then two `@(x, y): Footprint REF` lines.
fn kicad_overlaps(report: &str) -> BTreeSet<(String, String)> {
  let mut pairs = BTreeSet::new();
  let lines: Vec<&str> = report.lines().collect();
  for (i, line) in lines.iter().enumerate() {
    if !line.starts_with("[courtyards_overlap]") {
      continue;
    }
    let refs: Vec<String> = lines
      .iter()
      .skip(i + 1)
      .take(4)
      .filter_map(|l| l.split_once(": Footprint ").map(|(_, r)| r.trim().to_owned()))
      .take(2)
      .collect();
    if let [a, b] = refs.as_slice() {
      pairs.insert(if a <= b {
        (a.clone(), b.clone())
      } else {
        (b.clone(), a.clone())
      });
    }
  }
  pairs
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
