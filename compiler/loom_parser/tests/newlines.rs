//! Newline CST snapshot tests.

mod common;

#[test]
fn newlines_in_brackets() {
    loom_snapshot::assert_snapshot!(
        "newlines/newlines_in_brackets",
        common::render(
            "def f\n  x = [\n    1,\n    2,\n  ]\n  y = (\n    3 +\n    4\n  )\n  z = {\n    a =>\n    b\n  }\nend\n"
        )
    );
}

#[test]
fn newlines_and_leading_comments() {
    loom_snapshot::assert_snapshot!(
        "newlines/newlines_and_leading_comments",
        common::render("\n# leading comment\n\ndef f\nend\n")
    );
}
