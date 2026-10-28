//! Formatter tests: style rules on small inputs, and the guarantees of
//! `docs/reference/style.md` on every example.

use loom_span::FileId;
use loom_syntax::SyntaxKind;

use crate::format_source;

fn format(source: &str) -> String {
    format_source(source).unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"))
}

/// The significant tokens (kind and text), ignoring whitespace and line breaks.
fn significant_tokens(source: &str) -> Vec<(SyntaxKind, String)> {
    let parse = loom_parser::parse(source, FileId::DETACHED);
    parse
        .syntax()
        .tokens()
        .filter(|t| {
            !matches!(
                t.kind(),
                SyntaxKind::WHITESPACE | SyntaxKind::LINE_BREAK | SyntaxKind::NEWLINE
            )
        })
        .map(|t| (t.kind(), t.text().trim_end().to_owned()))
        .collect()
}

fn check_guarantees(name: &str, source: &str) {
    let once = format(source);
    let twice = format(&once);
    assert_eq!(once, twice, "{name}: formatting is not idempotent");
    assert_eq!(
        significant_tokens(source),
        significant_tokens(&once),
        "{name}: formatting changed the tokens"
    );
    assert!(
        !once.lines().any(|line| line.ends_with(' ')),
        "{name}: trailing whitespace"
    );
    assert!(
        once.ends_with('\n') && !once.ends_with("\n\n"),
        "{name}: file ending"
    );
    assert!(!once.contains("\n\n\n"), "{name}: double blank line");
}

#[test]
fn indentation_is_normalized() {
    let source = "def main\n      x = 1\n   if x > 0\n puts x\n        end\nend\n";
    assert_eq!(
        format(source),
        "def main\n  x = 1\n  if x > 0\n    puts x\n  end\nend\n"
    );
}

#[test]
fn spacing_is_normalized() {
    let source = "def f( a :Int,b:Int )->Int=a+b*-c\n";
    assert_eq!(
        format(source),
        "def f(a: Int, b: Int) -> Int = a + b * -c\n"
    );
    let formatted = format("def g\n  x = items [ 1 ] . bar ( k : 1 ) { | y |y }\nend\n");
    assert_eq!(
        formatted,
        "def g\n  x = items[1].bar(k: 1) { |y| y }\nend\n"
    );
    assert_eq!(
        format("def h\n  x = {  \"a\"=>1 , \"b\" => 2 }\nend\n"),
        "def h\n  x = {\"a\" => 1, \"b\" => 2}\nend\n"
    );
}

#[test]
fn ranges_closures_and_maps() {
    assert_eq!(
        format("def f\n  for i in 1 ..= 10\n  end\nend\n"),
        "def f\n  for i in 1..=10\n  end\nend\n"
    );
    assert_eq!(
        format("def f\n  g {||42}\n  h { |(a, b)|a+b }\nend\n"),
        "def f\n  g { || 42 }\n  h { |(a, b)| a + b }\nend\n"
    );
    assert_eq!(format("use a.b.{ C , d as e }\n"), "use a.b.{C, d as e}\n");
}

#[test]
fn case_arms_align_with_case() {
    let source = "def f(s: Shape) -> Int\n  case s\n    in Circle(r:) then 1\n    in Rect(..)\n      x = 2\n      x\n  else 3\n  end\nend\n";
    assert_eq!(
        format(source),
        "def f(s: Shape) -> Int\n  case s\n  in Circle(r:) then 1\n  in Rect(..)\n    x = 2\n    x\n  else 3\n  end\nend\n"
    );
}

#[test]
fn continuation_lines_indent_once() {
    let source = "def f\nx = a +\nb\nnames = users\n.filter { |u| u.active }\n.count\nend\n";
    assert_eq!(
        format(source),
        "def f\n  x = a +\n    b\n  names = users\n    .filter { |u| u.active }\n    .count\nend\n"
    );
}

#[test]
fn brackets_indent_once_per_line() {
    let source =
        "def f\n  shapes = [\n  Circle(radius: 1.0),\nRect(\n w: 2.0,\n h: 3.0,\n ),\n]\nend\n";
    assert_eq!(
        format(source),
        "def f\n  shapes = [\n    Circle(radius: 1.0),\n    Rect(\n      w: 2.0,\n      h: 3.0,\n    ),\n  ]\nend\n"
    );
}

#[test]
fn blank_lines_and_comments() {
    let source = "\n\n# top\ndef f\n\n  x = 1   # trailing\n\n\n\n  # about y\n  y = 2\n  # last\n\nend\n\n\n";
    assert_eq!(
        format(source),
        "# top\ndef f\n  x = 1   # trailing\n\n  # about y\n  y = 2\n  # last\nend\n"
    );
}

#[test]
fn struct_members_and_docs() {
    let source = "## A point.\n@[derive(Eq)]\nstruct P\nx: Int\n    ## The y.\n  pub y: Int\n\n def self.new -> P = P(x: 0, y: 0)\nend\n";
    assert_eq!(
        format(source),
        "## A point.\n@[derive(Eq)]\nstruct P\n  x: Int\n  ## The y.\n  pub y: Int\n\n  def self.new -> P = P(x: 0, y: 0)\nend\n"
    );
}

#[test]
fn strings_are_untouched() {
    let source = "def f\n      s = \"\"\"\n        keep   this\n          indent\n        \"\"\"\n  t = \"a  #{ x  +  1 }  b\"\nend\n";
    let formatted = format(source);
    assert!(formatted.contains("\n        keep   this\n          indent\n        \"\"\""));
    assert!(formatted.contains("\"a  #{x + 1}  b\""));
}

#[test]
fn files_with_errors_are_refused() {
    assert!(format_source("def main\n  x = (1\n").is_err());
}

#[test]
fn every_example_and_conformance_case_keeps_the_guarantees() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut directories = vec![format!("{root}/examples")];
    if let Ok(entries) = std::fs::read_dir(format!("{root}/spec/conformance")) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                directories.push(entry.path().display().to_string());
            }
        }
    }
    let mut checked = 0;
    for directory in directories {
        for entry in std::fs::read_dir(&directory).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "loom")
                && !path.with_extension("diag").exists()
            {
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue;
                };
                check_guarantees(&path.display().to_string(), &source);
                checked += 1;
            }
        }
    }
    assert!(checked >= 25);
}

#[test]
fn examples_are_already_formatted() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut unformatted = Vec::new();
    for entry in std::fs::read_dir(examples).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            let source = std::fs::read_to_string(&path).unwrap();
            if format(&source) != source {
                unformatted.push(path.display().to_string());
            }
        }
    }
    assert!(
        unformatted.is_empty(),
        "not in canonical style: {unformatted:?}"
    );
}
