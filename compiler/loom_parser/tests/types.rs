//! Type CST snapshot tests.

mod common;

#[test]
fn path_and_generics() {
    loom_snapshot::assert_snapshot!(
        "types/path_and_generics",
        common::render("def f(x: std.vec.Vector[Int])\nend\n")
    );
}

#[test]
fn tuple_type() {
    loom_snapshot::assert_snapshot!(
        "types/tuple_type",
        common::render("def f(x: (), y: (Int,), z: (Int, String))\nend\n")
    );
}

#[test]
fn fn_type() {
    loom_snapshot::assert_snapshot!(
        "types/fn_type",
        common::render("def f(a: Fn(Int) -> Bool, b: FnMut(), c: FnOnce(String))\nend\n")
    );
}

#[test]
fn dyn_type() {
    loom_snapshot::assert_snapshot!(
        "types/dyn_type",
        common::render("def f(x: dyn Display + Debug)\nend\n")
    );
}

#[test]
fn ref_type() {
    loom_snapshot::assert_snapshot!(
        "types/ref_type",
        common::render("def f(a: ref Int, b: mut ref String)\nend\n")
    );
}

#[test]
fn self_type() {
    loom_snapshot::assert_snapshot!("types/self_type", common::render("def f(x: Self)\nend\n"));
}
