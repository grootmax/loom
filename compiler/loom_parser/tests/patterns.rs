//! Pattern CST snapshot tests.

mod common;

#[test]
fn wildcard_and_lit() {
    loom_snapshot::assert_snapshot!(
        "patterns/wildcard_and_lit",
        common::render(
            "def f\n  case x\n  in _\n  in 1\n  in -1.5\n  in 'a'\n  in \"s\"\n  in true\n  end\nend\n"
        )
    );
}

#[test]
fn ident_binding() {
    loom_snapshot::assert_snapshot!(
        "patterns/ident_binding",
        common::render("def f\n  case x\n  in a\n  end\nend\n")
    );
}

#[test]
fn path_struct_tuple() {
    loom_snapshot::assert_snapshot!(
        "patterns/path_struct_tuple",
        common::render(
            "def f\n  case x\n  in Point(1, 2)\n  in Point(x: 1, y: 2)\n  in Point(..)\n  in Color.Red\n  end\nend\n"
        )
    );
}

#[test]
fn tuple_pattern() {
    loom_snapshot::assert_snapshot!(
        "patterns/tuple_pattern",
        common::render("def f\n  case x\n  in ()\n  in (a,)\n  in (a, b)\n  end\nend\n")
    );
}

#[test]
fn range_pattern() {
    loom_snapshot::assert_snapshot!(
        "patterns/range_pattern",
        common::render("def f\n  case x\n  in 1..5\n  in 'a'..='z'\n  end\nend\n")
    );
}

#[test]
fn or_pattern() {
    loom_snapshot::assert_snapshot!(
        "patterns/or_pattern",
        common::render("def f\n  case x\n  in 1 | 2 | 3\n  end\nend\n")
    );
}
