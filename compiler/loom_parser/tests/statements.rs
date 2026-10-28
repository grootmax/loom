//! Statement CST snapshot tests.

mod common;

#[test]
fn binding_var() {
    loom_snapshot::assert_snapshot!(
        "statements/binding_var",
        common::render("def f\n  var x: Int = 1\nend\n")
    );
}

#[test]
fn binding_let() {
    loom_snapshot::assert_snapshot!(
        "statements/binding_let",
        common::render("def f\n  x = 1\nend\n")
    );
}

#[test]
fn assignment_operators() {
    loom_snapshot::assert_snapshot!(
        "statements/assignment_operators",
        common::render("def f\n  x = 1\n  x += 2\n  self.a.b[0] *= 3\nend\n")
    );
}

#[test]
fn command_call_simple() {
    loom_snapshot::assert_snapshot!(
        "statements/command_call_simple",
        common::render("def f\n  puts \"hello\", 123, name: \"test\"\nend\n")
    );
}

#[test]
fn command_call_with_block_brace() {
    loom_snapshot::assert_snapshot!(
        "statements/command_call_with_block_brace",
        common::render("def f\n  route \"GET\", \"/path\" { |req| req.handle() }\nend\n")
    );
}

#[test]
fn command_call_with_block_do() {
    loom_snapshot::assert_snapshot!(
        "statements/command_call_with_block_do",
        common::render(
            "def f\n  route \"POST\", \"/path\" do |req|\n    req.handle()\n  end\nend\n"
        )
    );
}

#[test]
fn places() {
    loom_snapshot::assert_snapshot!(
        "statements/places",
        common::render(
            "def f\n  x = 1\n  @field = 2\n  self = 3\n  self.x.y[0] = 4\n  @a.b[1] = 5\nend\n"
        )
    );
}
