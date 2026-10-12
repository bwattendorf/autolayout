//! Lossless parser and writer for the KiCad s-expression format.
//!
//! The tree keeps every byte of the input: each node remembers the whitespace
//! that preceded it, and each list remembers the whitespace before its closing
//! paren. So `parse(s)?.to_string() == s` for any input that parses. That is the
//! guarantee the rest of autolayout leans on: an edit changes only the nodes it
//! touches, and KiCad sees the rest of the file exactly as it wrote it.
//!
//! Format reference: KiCad developer docs, "S-Expression Format",
//! <https://dev-docs.kicad.org/en/file-formats/sexpr-intro/>. Tokens are `(`,
//! `)`, double-quoted strings with backslash escapes, and bare atoms (any run of
//! characters that is not whitespace, a paren or a quote).
//!
//! Board files are untrusted input. Parsing is iterative (no recursion on input
//! depth), nesting is capped at [`MAX_DEPTH`], and every failure is a
//! [`ParseError`] with a byte offset and line, never a panic.

use std::borrow::Cow;
use std::fmt;

/// Deepest list nesting accepted. Real KiCad boards nest about a dozen levels;
/// this exists so a hostile file cannot drive the writer's recursion off the
/// stack.
pub const MAX_DEPTH: usize = 512;

/// A parsed file: its top-level nodes and any whitespace after the last one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
  pub items: Vec<Node>,
  pub tail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
  List(List),
  Atom(Atom),
}

/// `( items… )`. `lead` is the whitespace before `(`, `tail` the whitespace
/// before `)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct List {
  pub lead: String,
  pub items: Vec<Node>,
  pub tail: String,
}

/// A bare or quoted atom. `raw` is exactly the source text, quotes and escapes
/// included; [`Atom::value`] gives the decoded value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Atom {
  pub lead: String,
  raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
  pub offset: usize,
  pub line: usize,
  pub kind: ParseErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseErrorKind {
  UnexpectedClose,
  Unclosed { open_lists: usize },
  UnterminatedString,
  TooDeep,
}

/// Parse a whole file.
pub fn parse(src: &str) -> Result<Document, ParseError> {
  Parser { src, pos: 0 }.run()
}

impl Document {
  /// The first top-level list, which for a board is `(kicad_pcb …)`.
  pub fn root(&self) -> Option<&List> {
    self.items.iter().find_map(Node::as_list)
  }

  pub fn root_mut(&mut self) -> Option<&mut List> {
    self.items.iter_mut().find_map(Node::as_list_mut)
  }
}

impl Node {
  pub fn as_list(&self) -> Option<&List> {
    match self {
      Node::List(l) => Some(l),
      Node::Atom(_) => None,
    }
  }

  pub fn as_list_mut(&mut self) -> Option<&mut List> {
    match self {
      Node::List(l) => Some(l),
      Node::Atom(_) => None,
    }
  }

  pub fn as_atom(&self) -> Option<&Atom> {
    match self {
      Node::Atom(a) => Some(a),
      Node::List(_) => None,
    }
  }
}

impl List {
  /// The list's keyword: the value of its first item when that is an atom.
  pub fn head(&self) -> Option<Cow<'_, str>> {
    self.items.first().and_then(Node::as_atom).map(Atom::value)
  }

  pub fn is(&self, keyword: &str) -> bool {
    self.head().is_some_and(|h| h == keyword)
  }

  /// Direct child lists whose keyword is `keyword`.
  pub fn children<'a>(&'a self, keyword: &'a str) -> impl Iterator<Item = &'a List> + 'a {
    self
      .items
      .iter()
      .filter_map(Node::as_list)
      .filter(move |l| l.is(keyword))
  }

  pub fn children_mut<'a>(&'a mut self, keyword: &'a str) -> impl Iterator<Item = &'a mut List> + 'a {
    self
      .items
      .iter_mut()
      .filter_map(Node::as_list_mut)
      .filter(move |l| l.is(keyword))
  }

  pub fn child(&self, keyword: &str) -> Option<&List> {
    self
      .items
      .iter()
      .filter_map(Node::as_list)
      .find(|l| l.is(keyword))
  }

  /// Decoded value of the `i`-th item when it is an atom (0 is the keyword).
  pub fn atom(&self, i: usize) -> Option<Cow<'_, str>> {
    self.items.get(i).and_then(Node::as_atom).map(Atom::value)
  }

  /// The `i`-th item parsed as a finite number.
  pub fn number(&self, i: usize) -> Option<f64> {
    self.atom(i)?.parse::<f64>().ok().filter(|v| v.is_finite())
  }
}

impl Atom {
  /// A bare atom. Returns `None` if `text` would not survive as one (empty, or
  /// containing whitespace, a paren or a quote) — use [`Atom::quoted`] then.
  pub fn bare(lead: impl Into<String>, text: &str) -> Option<Atom> {
    let ok = !text.is_empty() && text.chars().all(is_bare_char);
    ok.then(|| Atom {
      lead: lead.into(),
      raw: text.to_owned(),
    })
  }

  /// A quoted atom holding `value`, escaped so it reads back identically.
  pub fn quoted(lead: impl Into<String>, value: &str) -> Atom {
    let mut raw = String::with_capacity(value.len() + 2);
    raw.push('"');
    for c in value.chars() {
      match c {
        '"' => raw.push_str("\\\""),
        '\\' => raw.push_str("\\\\"),
        '\n' => raw.push_str("\\n"),
        '\r' => raw.push_str("\\r"),
        '\t' => raw.push_str("\\t"),
        c => raw.push(c),
      }
    }
    raw.push('"');
    Atom {
      lead: lead.into(),
      raw,
    }
  }

  /// Source text exactly as written.
  pub fn raw(&self) -> &str {
    &self.raw
  }

  pub fn is_quoted(&self) -> bool {
    self.raw.starts_with('"')
  }

  /// Decoded value: quotes stripped and escapes resolved for a quoted atom,
  /// the text itself for a bare one.
  pub fn value(&self) -> Cow<'_, str> {
    let Some(inner) = self.raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
      return Cow::Borrowed(&self.raw);
    };
    if !inner.contains('\\') {
      return Cow::Borrowed(inner);
    }
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
      if c != '\\' {
        out.push(c);
        continue;
      }
      match chars.next() {
        Some('n') => out.push('\n'),
        Some('r') => out.push('\r'),
        Some('t') => out.push('\t'),
        Some(other) => out.push(other),
        None => out.push('\\'),
      }
    }
    Cow::Owned(out)
  }
}

impl fmt::Display for Document {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for item in &self.items {
      item.fmt(f)?;
    }
    f.write_str(&self.tail)
  }
}

impl fmt::Display for Node {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Node::List(l) => l.fmt(f),
      Node::Atom(a) => a.fmt(f),
    }
  }
}

impl fmt::Display for List {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.lead)?;
    f.write_str("(")?;
    for item in &self.items {
      item.fmt(f)?;
    }
    f.write_str(&self.tail)?;
    f.write_str(")")
  }
}

impl fmt::Display for Atom {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&self.lead)?;
    f.write_str(&self.raw)
  }
}

impl fmt::Display for ParseError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let what = match &self.kind {
      ParseErrorKind::UnexpectedClose => "unexpected ')'".to_owned(),
      ParseErrorKind::Unclosed { open_lists } => {
        format!("end of input with {open_lists} list(s) still open")
      }
      ParseErrorKind::UnterminatedString => "unterminated string".to_owned(),
      ParseErrorKind::TooDeep => format!("nesting deeper than {MAX_DEPTH}"),
    };
    write!(f, "line {}, byte {}: {what}", self.line, self.offset)
  }
}

impl std::error::Error for ParseError {}

fn is_space(c: char) -> bool {
  matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

fn is_bare_char(c: char) -> bool {
  !is_space(c) && !matches!(c, '(' | ')' | '"')
}

struct Parser<'a> {
  src: &'a str,
  pos: usize,
}

impl Parser<'_> {
  fn run(mut self) -> Result<Document, ParseError> {
    let mut doc = Document::default();
    let mut open: Vec<List> = Vec::new();
    let mut lead = String::new();

    while let Some(c) = self.peek() {
      if is_space(c) {
        lead.push(c);
        self.pos += c.len_utf8();
        continue;
      }
      let node_start = self.pos;
      match c {
        '(' => {
          if open.len() >= MAX_DEPTH {
            return Err(self.error(node_start, ParseErrorKind::TooDeep));
          }
          self.pos += 1;
          open.push(List {
            lead: std::mem::take(&mut lead),
            items: Vec::new(),
            tail: String::new(),
          });
        }
        ')' => {
          let Some(mut list) = open.pop() else {
            return Err(self.error(node_start, ParseErrorKind::UnexpectedClose));
          };
          self.pos += 1;
          list.tail = std::mem::take(&mut lead);
          push(&mut open, &mut doc, Node::List(list));
        }
        '"' => {
          let end = self
            .string_end()
            .ok_or_else(|| self.error(node_start, ParseErrorKind::UnterminatedString))?;
          let raw = self.slice(node_start, end).to_owned();
          self.pos = end;
          push(
            &mut open,
            &mut doc,
            Node::Atom(Atom {
              lead: std::mem::take(&mut lead),
              raw,
            }),
          );
        }
        _ => {
          let end = self.bare_end();
          let raw = self.slice(node_start, end).to_owned();
          self.pos = end;
          push(
            &mut open,
            &mut doc,
            Node::Atom(Atom {
              lead: std::mem::take(&mut lead),
              raw,
            }),
          );
        }
      }
    }

    if !open.is_empty() {
      return Err(self.error(
        self.pos,
        ParseErrorKind::Unclosed {
          open_lists: open.len(),
        },
      ));
    }
    doc.tail = lead;
    Ok(doc)
  }

  fn peek(&self) -> Option<char> {
    self.src.get(self.pos..).and_then(|s| s.chars().next())
  }

  fn slice(&self, start: usize, end: usize) -> &str {
    self.src.get(start..end).unwrap_or_default()
  }

  /// Byte offset just past the closing quote of the string starting at `pos`.
  fn string_end(&self) -> Option<usize> {
    let rest = self.src.get(self.pos + 1..)?;
    let mut escaped = false;
    for (i, c) in rest.char_indices() {
      if escaped {
        escaped = false;
      } else if c == '\\' {
        escaped = true;
      } else if c == '"' {
        return Some(self.pos + 1 + i + 1);
      }
    }
    None
  }

  fn bare_end(&self) -> usize {
    let rest = self.src.get(self.pos..).unwrap_or_default();
    match rest.char_indices().find(|&(_, c)| !is_bare_char(c)) {
      Some((i, _)) => self.pos + i,
      None => self.src.len(),
    }
  }

  fn error(&self, offset: usize, kind: ParseErrorKind) -> ParseError {
    let line = 1 + self.src.get(..offset).unwrap_or_default().matches('\n').count();
    ParseError { offset, line, kind }
  }
}

fn push(open: &mut [List], doc: &mut Document, node: Node) {
  match open.last_mut() {
    Some(parent) => parent.items.push(node),
    None => doc.items.push(node),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const SAMPLE: &str = "(kicad_pcb (version 20240108) (generator \"pcbnew\")\n\t(footprint \"R_0603\"\n\t\t(at 10.5 -3 90)\n\t\t(property \"Reference\" \"R1\")\n\t)\n)\n";

  mod round_trip {
    use super::*;

    #[test]
    fn sample_is_byte_exact() {
      assert_eq!(parse(SAMPLE).unwrap().to_string(), SAMPLE);
    }

    #[test]
    fn odd_whitespace_is_kept() {
      for s in [
        "",
        "  \n",
        "( )",
        "(a(b)c)",
        "\r\n(a\r\n  b )\r\n\r\n",
        "(\"\")",
        "(a\"b\"c)",
        "x y (z)",
      ] {
        assert_eq!(parse(s).unwrap().to_string(), s, "input {s:?}");
      }
    }

    #[test]
    fn escapes_survive() {
      let s = r#"(text "say \"hi\"\\ok\n")"#;
      let doc = parse(s).unwrap();
      assert_eq!(doc.to_string(), s);
      let text = doc.root().unwrap().atom(1).unwrap();
      assert_eq!(text, "say \"hi\"\\ok\n");
    }

    #[test]
    fn quoted_constructor_reads_back() {
      let value = "a \"b\" \\ c\nd";
      let atom = Atom::quoted(" ", value);
      let doc = parse(&format!("(x{atom})")).unwrap();
      assert_eq!(doc.root().unwrap().atom(1).unwrap(), value);
    }
  }

  mod access {
    use super::*;

    #[test]
    fn head_children_numbers() {
      let doc = parse(SAMPLE).unwrap();
      let root = doc.root().unwrap();
      assert!(root.is("kicad_pcb"));
      let fp = root.child("footprint").unwrap();
      assert_eq!(fp.atom(1).unwrap(), "R_0603");
      let at = fp.child("at").unwrap();
      assert_eq!(
        (at.number(1), at.number(2), at.number(3)),
        (Some(10.5), Some(-3.0), Some(90.0))
      );
      assert_eq!(fp.children("property").count(), 1);
    }

    #[test]
    fn non_finite_numbers_are_rejected() {
      let doc = parse("(at inf NaN 1e999)").unwrap();
      let at = doc.root().unwrap();
      assert_eq!((at.number(1), at.number(2), at.number(3)), (None, None, None));
    }

    #[test]
    fn bare_refuses_unsafe_text() {
      assert!(Atom::bare("", "ok_1.5").is_some());
      for bad in ["", "a b", "a(b", "a\"b"] {
        assert!(Atom::bare("", bad).is_none(), "{bad:?}");
      }
    }
  }

  mod errors {
    use super::*;

    fn kind(s: &str) -> ParseErrorKind {
      parse(s).unwrap_err().kind
    }

    #[test]
    fn malformed_input_is_an_error() {
      assert_eq!(kind(")"), ParseErrorKind::UnexpectedClose);
      assert_eq!(kind("(a (b)"), ParseErrorKind::Unclosed { open_lists: 1 });
      assert_eq!(kind("(a \"open"), ParseErrorKind::UnterminatedString);
      assert_eq!(kind("(a \"esc\\\""), ParseErrorKind::UnterminatedString);
    }

    #[test]
    fn deep_nesting_is_capped() {
      let deep = "(".repeat(MAX_DEPTH + 1) + &")".repeat(MAX_DEPTH + 1);
      assert_eq!(kind(&deep), ParseErrorKind::TooDeep);
      let ok = "(".repeat(MAX_DEPTH) + &")".repeat(MAX_DEPTH);
      assert_eq!(parse(&ok).unwrap().to_string(), ok);
    }

    #[test]
    fn error_reports_line() {
      let err = parse("(a\n(b\n)\n)\n)").unwrap_err();
      assert_eq!(err.line, 5);
    }
  }
}
