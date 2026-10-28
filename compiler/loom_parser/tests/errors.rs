//! Error CST snapshot tests.

mod common;

#[test]
fn e0100_expected_token() {
    loom_snapshot::assert_snapshot!("errors/e0100_expected_token", common::render("def\n"));
}

#[test]
fn e0101_unclosed_delimiter() {
    loom_snapshot::assert_snapshot!(
        "errors/e0101_unclosed_delimiter",
        common::render("def f\n  x = (1 + 2\nend\n")
    );
}

#[test]
fn e0102_missing_end() {
    loom_snapshot::assert_snapshot!(
        "errors/e0102_missing_end",
        common::render("def f\n  x = 1\n")
    );
}

#[test]
fn e0103_unexpected_closing() {
    loom_snapshot::assert_snapshot!(
        "errors/e0103_unexpected_closing",
        common::render("def f\n  x = 1 )\nend\n")
    );
}

#[test]
fn e0104_chained_comparison() {
    loom_snapshot::assert_snapshot!(
        "errors/e0104_chained_comparison",
        common::render("def f\n  res = a < b < c\nend\n")
    );
}

#[test]
fn e0105_chained_range() {
    loom_snapshot::assert_snapshot!(
        "errors/e0105_chained_range",
        common::render("def f\n  res = a .. b .. c\nend\n")
    );
}

#[test]
fn e0106_assignment_in_expression() {
    loom_snapshot::assert_snapshot!(
        "errors/e0106_assignment_in_expression",
        common::render("def f\n  if x = 1 then\n  end\nend\n")
    );
}

#[test]
fn e0107_positional_after_named() {
    loom_snapshot::assert_snapshot!(
        "errors/e0107_positional_after_named",
        common::render("def f\n  g(a: 1, 2)\nend\n")
    );
}

#[test]
fn e0108_misplaced_inner_doc() {
    loom_snapshot::assert_snapshot!(
        "errors/e0108_misplaced_inner_doc",
        common::render("def f\nend\n##! misplaced inner doc\n")
    );
}

#[test]
fn e0109_inherent_impl() {
    loom_snapshot::assert_snapshot!(
        "errors/e0109_inherent_impl",
        common::render("impl Foo\n  def bar\n  end\nend\n")
    );
}

#[test]
fn e0110_else_if() {
    loom_snapshot::assert_snapshot!(
        "errors/e0110_else_if",
        common::render("def f\n  if a then\n    1\n  else if b then\n    2\n  end\nend\n")
    );
}

#[test]
fn e0111_statements_on_one_line() {
    loom_snapshot::assert_snapshot!(
        "errors/e0111_statements_on_one_line",
        common::render("def f\n  x = 1 y = 2\nend\n")
    );
}

#[test]
fn e0112_nesting_too_deep() {
    let source = format!(
        "def f\n  x = {}1{}\nend\n",
        "(".repeat(300),
        ")".repeat(300)
    );
    loom_snapshot::assert_snapshot!("errors/e0112_nesting_too_deep", common::render(&source));
}

#[test]
fn e0113_glob_import() {
    loom_snapshot::assert_snapshot!("errors/e0113_glob_import", common::render("use foo.*\n"));
}

#[test]
fn recovery_item_after_error() {
    loom_snapshot::assert_snapshot!(
        "errors/recovery_item_after_error",
        common::render("def bad(a:)\nend\n\ndef good\n  x = 1\nend\n")
    );
}
