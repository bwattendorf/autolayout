//! Deterministic fuzzing of the parser (M0.2).
//!
//! Board files are untrusted input. Two properties, on every generated case:
//! 1. `parse` returns — `Ok` or `Err` — and never panics;
//! 2. whenever it returns `Ok`, writing the tree back reproduces the input
//!    byte for byte.
//!
//! Cases are generated from a fixed seed with a small xorshift generator, so a
//! failure prints a case index that reproduces exactly. No dependencies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use autolayout_sexpr::{ParseErrorKind, parse};

const SEED: u64 = 0x5eed_2026_1012;
const CASES: usize = 20_000;

/// A realistic fragment to mutate: the shapes a board file is made of.
const SEED_TEXT: &str = "(kicad_pcb (version 20241229) (generator \"pcbnew\")\n\t(net 3 \"+3V3\")\n\t(footprint \"Resistor_SMD:R_0603\"\n\t\t(layer \"F.Cu\")\n\t\t(at 74.9 88.8 90)\n\t\t(property \"Reference\" \"R8\" (at 0 2 270))\n\t\t(pad \"1\" smd roundrect (at -0.825 0 90) (size 0.8 0.95) (net 3 \"+3V3\"))\n\t\t(fp_text user \"say \\\"hi\\\" \\\\ ok\" (at 0 0))\n\t)\n)\n";

/// Characters that matter to the tokenizer, weighted toward the dangerous ones.
const ALPHABET: &[char] = &[
  '(', ')', '"', '\\', ' ', '\t', '\n', '\r', 'a', '1', '.', '-', 'é', '中',
];

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    x
  }

  fn below(&mut self, n: usize) -> usize {
    if n == 0 {
      0
    } else {
      (self.next() % n as u64) as usize
    }
  }
}

#[test]
fn parser_never_panics_and_accepted_input_round_trips() {
  let mut rng = Rng(SEED);
  let seed_chars: Vec<char> = SEED_TEXT.chars().collect();
  let (mut accepted, mut rejected) = (0, 0);

  for case in 0..CASES {
    let input = match case % 4 {
      // pure noise from the dangerous alphabet
      0 => (0..rng.below(200))
        .map(|_| ALPHABET[rng.below(ALPHABET.len())])
        .collect::<String>(),
      // a truncated real fragment
      1 => seed_chars[..rng.below(seed_chars.len() + 1)].iter().collect(),
      // a real fragment with a few characters replaced
      2 => {
        let mut c = seed_chars.clone();
        for _ in 0..1 + rng.below(4) {
          let i = rng.below(c.len());
          c[i] = ALPHABET[rng.below(ALPHABET.len())];
        }
        c.into_iter().collect()
      }
      // two random slices of the fragment spliced together
      _ => {
        let (a, b) = (rng.below(seed_chars.len()), rng.below(seed_chars.len()));
        seed_chars[..a].iter().chain(&seed_chars[b..]).collect()
      }
    };

    let outcome = std::panic::catch_unwind(|| parse(&input).map(|doc| doc.to_string()));
    match outcome {
      Err(_) => panic!("case {case}: parser panicked on {input:?}"),
      Ok(Ok(written)) => {
        assert_eq!(written, input, "case {case}: accepted but not byte-exact");
        accepted += 1;
      }
      Ok(Err(_)) => rejected += 1,
    }
  }
  eprintln!("fuzz: {CASES} cases, {accepted} accepted (all byte-exact), {rejected} rejected, 0 panics");
  assert!(
    accepted > 100 && rejected > 100,
    "generator is not exercising both paths"
  );
}

#[test]
fn pathological_depth_is_an_error_not_a_stack_overflow() {
  let deep = "(".repeat(1_000_000);
  assert_eq!(parse(&deep).unwrap_err().kind, ParseErrorKind::TooDeep);
}

#[test]
fn huge_flat_input_parses() {
  let wide = format!("({})", "(a) ".repeat(200_000));
  let doc = parse(&wide).unwrap();
  assert_eq!(doc.root().unwrap().items.len(), 200_000);
  assert_eq!(doc.to_string(), wide);
}
