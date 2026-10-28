//! Item CST snapshot tests.

mod common;

#[test]
fn fn_single_expression() {
    loom_snapshot::assert_snapshot!(
        "items/fn_single_expression",
        common::render("def add(a: Int, b: Int) -> Int = a + b\n")
    );
}

#[test]
fn fn_block_body() {
    loom_snapshot::assert_snapshot!(
        "items/fn_block_body",
        common::render("def main\n  puts \"hello\"\nend\n")
    );
}

#[test]
fn fn_receiver_modes() {
    loom_snapshot::assert_snapshot!(
        "items/fn_receiver_modes",
        common::render("def mut self.update()\nend\n\ndef own self.consume()\nend\n")
    );
}

#[test]
fn fn_associated() {
    loom_snapshot::assert_snapshot!(
        "items/fn_associated",
        common::render("def self.new() -> Self\n  Self()\nend\n")
    );
}

#[test]
fn fn_generic_and_where() {
    loom_snapshot::assert_snapshot!(
        "items/fn_generic_and_where",
        common::render("def f[T](x: T) -> T where T: Copy\n  x\nend\n")
    );
}

#[test]
fn fn_param_default() {
    loom_snapshot::assert_snapshot!(
        "items/fn_param_default",
        common::render("def f(a: Int = 0, b: String = \"hi\")\nend\n")
    );
}

#[test]
fn struct_def() {
    loom_snapshot::assert_snapshot!(
        "items/struct_def",
        common::render(
            "struct Point[T] where T: Copy\n  x: T\n  y: T\n  @[inline]\n  pub def clear\n  end\nend\n"
        )
    );
}

#[test]
fn enum_def() {
    loom_snapshot::assert_snapshot!(
        "items/enum_def",
        common::render(
            "enum Option[T]\n  Some(T)\n  None\n  pub def is_some -> Bool = true\nend\n"
        )
    );
}

#[test]
fn enum_variant_named() {
    loom_snapshot::assert_snapshot!(
        "items/enum_variant_named",
        common::render("enum Shape\n  Circle(radius: Float)\n  Rect(w: Float, h: Float)\nend\n")
    );
}

#[test]
fn trait_def() {
    loom_snapshot::assert_snapshot!(
        "items/trait_def",
        common::render(
            "trait Printable[T] : Base where T: Display\n  def print(self: ref Self)\n  default def name -> String = \"item\"\n  type Assoc: Bounds\n  const MIN: Int = 0\nend\n"
        )
    );
}

#[test]
fn impl_def() {
    loom_snapshot::assert_snapshot!(
        "items/impl_def",
        common::render(
            "impl[T] Display for Point[T] where T: Display\n  def fmt(self: ref Self) -> String = \"point\"\n  type Assoc = Int\n  const VAL: Int = 1\nend\n"
        )
    );
}

#[test]
fn module_def() {
    loom_snapshot::assert_snapshot!(
        "items/module_def",
        common::render("module Inner\n  def f = 1\nend\n")
    );
}

#[test]
fn use_decl() {
    loom_snapshot::assert_snapshot!(
        "items/use_decl",
        common::render("use a.b.c as d\nuse x.y.{a, b.c}\n")
    );
}

#[test]
fn const_def() {
    loom_snapshot::assert_snapshot!("items/const_def", common::render("const MAX: Int = 100\n"));
}

#[test]
fn type_alias() {
    loom_snapshot::assert_snapshot!("items/type_alias", common::render("type MyInt[T] = Int\n"));
}

#[test]
fn extern_block() {
    loom_snapshot::assert_snapshot!(
        "items/extern_block",
        common::render("extern \"C\"\n  def puts(s: String) -> Int\nend\n")
    );
}

#[test]
fn attributes_and_visibility() {
    loom_snapshot::assert_snapshot!(
        "items/attributes_and_visibility",
        common::render("@[inline, target(\"x86\")]\npub(package) def f = 1\n")
    );
}

#[test]
fn doc_comment_attached() {
    loom_snapshot::assert_snapshot!(
        "items/doc_comment_attached",
        common::render("## Outer doc comment for f\ndef f\nend\n")
    );
}
