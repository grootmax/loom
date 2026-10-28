//! Parser tests: every example parses without errors and losslessly.

use loom_span::FileId;

use crate::parse;

#[test]
fn every_example_parses_cleanly_and_losslessly() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut failures = Vec::new();
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "loom") {
            let source = std::fs::read_to_string(&path).unwrap();
            let parse = parse(&source, FileId::DETACHED);
            assert_eq!(parse.syntax().text(), source, "{}", path.display());
            for diagnostic in parse.diagnostics() {
                let line = source[..diagnostic.primary.span.range.start().to_usize()]
                    .lines()
                    .count();
                failures.push(format!("{}:{line}: {}", path.display(), diagnostic.message));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn codes(source: &str) -> Vec<String> {
    parse(source, FileId::DETACHED)
        .diagnostics()
        .iter()
        .map(|d| d.id.to_string())
        .collect()
}

#[test]
fn unterminated_literal_does_not_cascade_at_end_of_file() {
    assert_eq!(codes("def main\n  puts \"oops\n  x = 1\nend\n"), ["E0018"]);
    assert_eq!(codes("def main\n  f(r#\"oops\n"), ["E0026"]);
}

#[test]
fn recovery_keeps_later_items() {
    // An error in one function must not hide the next one.
    let parse = parse(
        "def a\n  x = (1 +\nend\n\ndef b\n  puts 2\nend\n",
        FileId::DETACHED,
    );
    let names: Vec<String> = parse
        .syntax()
        .descendants()
        .filter(|n| n.kind() == loom_syntax::SyntaxKind::FN_DEF)
        .map(|n| n.text())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names[1].starts_with("def b"));
}

#[test]
fn foreign_operators_recover_silently() {
    assert_eq!(
        codes("def f\n  if !a && b || c\n    x = 1; y = 2\n  end\nend\n"),
        ["E0007", "E0008", "E0008", "E0006"]
    );
}

#[test]
fn deep_nesting_is_reported_not_overflowed() {
    let source = format!(
        "def f\n  x = {}1{}\nend\n",
        "(".repeat(5000),
        ")".repeat(5000)
    );
    assert!(codes(&source).contains(&"E0112".to_owned()));
    assert_eq!(parse(&source, FileId::DETACHED).syntax().text(), source);
}

#[test]
fn binary_precedence_follows_spec_7_5() {
    // Each operator one level tighter than its left neighbor must bind first.
    let cases = [
        ("1 + 2 * 3", "1 + (2 * 3)"),
        ("1 << 2 + 1", "1 << (2 + 1)"),
        ("a or b and c", "a or (b and c)"),
        ("a & b | c ^ d", "(a & b) | (c ^ d)"),
        ("1 - 2 - 3", "(1 - 2) - 3"),
        ("a == b + 1", "a == (b + 1)"),
        ("0..n + 1", "0..(n + 1)"),
    ];
    for (source, expected) in cases {
        let parse = crate::parse(
            &format!("def f\n  x = {source}\nend\n"),
            loom_span::FileId::DETACHED,
        );
        assert!(!parse.has_errors(), "{source}");
        let tree = parse.syntax();
        let expr = tree
            .descendants()
            .find(|n| {
                matches!(
                    n.kind(),
                    loom_syntax::SyntaxKind::BINARY_EXPR | loom_syntax::SyntaxKind::RANGE_EXPR
                )
            })
            .expect("binary expression");
        assert_eq!(parenthesize(&expr), expected, "{source}");
    }
}

/// Renders a binary tree with parentheses around nested binary operands.
fn parenthesize(node: &loom_syntax::SyntaxNode) -> String {
    use loom_syntax::SyntaxKind::{BINARY_EXPR, RANGE_EXPR};
    let mut parts = Vec::new();
    for child in node.children_with_tokens() {
        match child {
            loom_syntax::SyntaxElement::Node(n) if matches!(n.kind(), BINARY_EXPR | RANGE_EXPR) => {
                parts.push(format!("({})", parenthesize(&n)));
            }
            loom_syntax::SyntaxElement::Node(n) => parts.push(n.text().trim().to_owned()),
            loom_syntax::SyntaxElement::Token(t) if !t.kind().is_trivia() => {
                parts.push(t.text().to_owned());
            }
            loom_syntax::SyntaxElement::Token(_) => {}
        }
    }
    let joined = parts.join(" ");
    joined
        .replace(" .. ", "..")
        .replace("( ", "(")
        .replace(" )", ")")
}
