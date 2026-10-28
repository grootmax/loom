//! Tests for diagnostic snippet renderer.

use loom_diagnostics::codes;
use loom_diagnostics::render::{RenderOptions, render};
use loom_diagnostics::{Applicability, Diagnostic, Suggestion};
use loom_span::{FileId, SourceMap, Span, TextRange, TextSize};

#[test]
fn test_single_primary_label() {
    let mut sources = SourceMap::new();
    let text = "def main\n  if !ready\nend\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(14), TextSize::new(15)));
    let diag = Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
        .with_label("not an operator");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0007]: `!` is not an operator
 --> src/main.loom:2:6
  |
2 |   if !ready
  |      ^ not an operator
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_notes_help_and_suggestions() {
    let mut sources = SourceMap::new();
    let text = "def main\n  if !ready\nend\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(14), TextSize::new(15)));
    let diag = Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
        .with_label("not an operator")
        .with_note("first note")
        .with_help("use `not` for logical negation")
        .with_suggestion(Suggestion::replace(
            "use `not`",
            span,
            "not ",
            Applicability::MachineApplicable,
        ));

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0007]: `!` is not an operator
 --> src/main.loom:2:6
  |
2 |   if !ready
  |      ^ not an operator
  |
  = note: first note
  = help: use `not` for logical negation
  = help: use `not`
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_secondary_label_another_line_with_gap() {
    let mut sources = SourceMap::new();
    let text = "def first\n  x\nend\n\ndef second\n  y\nend\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let sec_span = Span::new(file, TextRange::new(TextSize::new(0), TextSize::new(3)));
    let pri_span = Span::new(file, TextRange::new(TextSize::new(32), TextSize::new(33)));

    let diag = Diagnostic::error(codes::EXPECTED_TOKEN, pri_span, "redefinition of `y`")
        .with_label("redefinition here")
        .with_secondary(sec_span, "first defined here");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: redefinition of `y`
 --> src/main.loom:6:3
  |
1 | def first
  | --- first defined here
...
6 |   y
  |   ^ redefinition here
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_secondary_label_same_line() {
    let mut sources = SourceMap::new();
    let text = "val x = a + b\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let sec_span = Span::new(file, TextRange::new(TextSize::new(8), TextSize::new(9)));
    let pri_span = Span::new(file, TextRange::new(TextSize::new(12), TextSize::new(13)));

    let diag = Diagnostic::error(codes::EXPECTED_TOKEN, pri_span, "type mismatch")
        .with_label("type mismatch")
        .with_secondary(sec_span, "expected integer");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: type mismatch
 --> src/main.loom:1:13
  |
1 | val x = a + b
  |         - expected integer
  |             ^ type mismatch
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_multiline_span() {
    let mut sources = SourceMap::new();
    let text = "def foo\n  let x = 1\n  let y = 2\nend\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(10), TextSize::new(29)));
    let diag =
        Diagnostic::error(codes::EXPECTED_TOKEN, span, "invalid block").with_label("invalid block");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: invalid block
 --> src/main.loom:2:3
  |
2 |   let x = 1
  |   ^^^^^^^^^ ... invalid block
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_tab_before_span() {
    let mut sources = SourceMap::new();
    let text = "\tif !ready\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(4), TextSize::new(5)));
    let diag = Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
        .with_label("not an operator");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0007]: `!` is not an operator
 --> src/main.loom:1:5
  |
1 |     if !ready
  |        ^ not an operator
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_span_at_end_of_file() {
    let mut sources = SourceMap::new();
    let text = "abc";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(3), TextSize::new(3)));
    let diag = Diagnostic::error(codes::EXPECTED_TOKEN, span, "unexpected end of file")
        .with_label("unexpected end of file");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: unexpected end of file
 --> src/main.loom:1:4
  |
1 | abc
  |    ^ unexpected end of file
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_warning() {
    let mut sources = SourceMap::new();
    let text = "## doc\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(0), TextSize::new(6)));
    let diag = Diagnostic::warning(
        codes::DANGLING_DOC_COMMENT,
        span,
        "doc comment is not followed by an item",
    );

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
warning[dangling_doc_comment]: doc comment is not followed by an item
 --> src/main.loom:1:1
  |
1 | ## doc
  | ^^^^^^
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_detached_file() {
    let sources = SourceMap::new();
    let span = Span::new(
        FileId::DETACHED,
        TextRange::new(TextSize::new(0), TextSize::new(0)),
    );
    let diag = Diagnostic::error(codes::EXPECTED_TOKEN, span, "missing semicolon")
        .with_note("detached note");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: missing semicolon
  |
  = note: detached note
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_two_digit_line_numbers() {
    use std::fmt::Write as _;
    let mut sources = SourceMap::new();
    let mut text = String::new();
    for i in 1..=11 {
        let _ = writeln!(text, "line {i}");
    }
    let line12_offset = text.len();
    text.push_str("  let x = 1\n");
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(
        file,
        TextRange::new(
            TextSize::new(u32::try_from(line12_offset).unwrap() + 2),
            TextSize::new(u32::try_from(line12_offset).unwrap() + 5),
        ),
    );
    let diag =
        Diagnostic::error(codes::EXPECTED_TOKEN, span, "unused variable").with_label("unused");

    let options = RenderOptions { color: false };
    let rendered = render(&diag, &sources, &options);

    let expected = "\
error[E0100]: unused variable
  --> src/main.loom:12:3
   |
12 |   let x = 1
   |   ^^^ unused
";
    assert_eq!(rendered, expected);
}

#[test]
fn test_color_on() {
    let mut sources = SourceMap::new();
    let text = "if !ready\n";
    let file = sources.add("src/main.loom", text).unwrap();
    let span = Span::new(file, TextRange::new(TextSize::new(3), TextSize::new(4)));
    let diag = Diagnostic::error(codes::BANG_OPERATOR, span, "`!` is not an operator")
        .with_label("not an operator");

    let options = RenderOptions { color: true };
    let rendered = render(&diag, &sources, &options);

    let expected = "\x1b[1;31merror[E0007]\x1b[0m: \x1b[1m`!` is not an operator\x1b[0m\n \x1b[1;34m-->\x1b[0m src/main.loom:1:4\n  \x1b[1;34m|\x1b[0m\n\x1b[1;34m1\x1b[0m \x1b[1;34m|\x1b[0m if !ready\n  \x1b[1;34m|\x1b[0m    \x1b[1;31m^ not an operator\x1b[0m\n";
    assert_eq!(rendered, expected);
}
