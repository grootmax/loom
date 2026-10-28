//! String CST snapshot tests.

mod common;

#[test]
fn char_and_string() {
    loom_snapshot::assert_snapshot!(
        "strings/char_and_string",
        common::render("def f\n  ch = 'a'\n  str = \"hello\\nworld\"\nend\n")
    );
}

#[test]
fn triple_quote_string() {
    loom_snapshot::assert_snapshot!(
        "strings/triple_quote_string",
        common::render("def f\n  s = \"\"\"\n  line 1\n  line 2\n  \"\"\"\nend\n")
    );
}

#[test]
fn raw_string() {
    loom_snapshot::assert_snapshot!(
        "strings/raw_string",
        common::render("def f\n  r = r#\"raw \"string\" #\"#\nend\n")
    );
}

#[test]
fn byte_string() {
    loom_snapshot::assert_snapshot!(
        "strings/byte_string",
        common::render("def f\n  b = b\"hello\"\n  br = br#\"byte raw\"#\nend\n")
    );
}

#[test]
fn nested_interpolation() {
    loom_snapshot::assert_snapshot!(
        "strings/nested_interpolation",
        common::render("def f\n  s = \"a #{ \"b #{c}\" } d\"\nend\n")
    );
}
