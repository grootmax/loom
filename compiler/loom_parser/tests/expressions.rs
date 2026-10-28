//! Expression CST snapshot tests.

mod common;

#[test]
fn precedence_or_and() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_or_and",
        common::render("def f\n  a or b and c\nend\n")
    );
}

#[test]
fn precedence_and_not() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_and_not",
        common::render("def f\n  a and not b\nend\n")
    );
}

#[test]
fn precedence_not_cmp() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_not_cmp",
        common::render("def f\n  not a == b\nend\n")
    );
}

#[test]
fn precedence_cmp_range() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_cmp_range",
        common::render("def f\n  a == b..c\nend\n")
    );
}

#[test]
fn precedence_range_bitor() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_range_bitor",
        common::render("def f\n  a | b .. c | d\nend\n")
    );
}

#[test]
fn precedence_bitor_bitxor() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_bitor_bitxor",
        common::render("def f\n  a | b ^ c\nend\n")
    );
}

#[test]
fn precedence_bitxor_bitand() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_bitxor_bitand",
        common::render("def f\n  a ^ b & c\nend\n")
    );
}

#[test]
fn precedence_bitand_shift() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_bitand_shift",
        common::render("def f\n  a & b << c\nend\n")
    );
}

#[test]
fn precedence_shift_add() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_shift_add",
        common::render("def f\n  a << b + c\nend\n")
    );
}

#[test]
fn precedence_add_mul() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_add_mul",
        common::render("def f\n  a + b * c\nend\n")
    );
}

#[test]
fn precedence_mul_unary() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_mul_unary",
        common::render("def f\n  -a * b\nend\n")
    );
}

#[test]
fn precedence_unary_postfix() {
    loom_snapshot::assert_snapshot!(
        "expressions/precedence_unary_postfix",
        common::render("def f\n  -a.b\nend\n")
    );
}

#[test]
fn range_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/range_expr",
        common::render("def f\n  1..2\n  1..=2\nend\n")
    );
}

#[test]
fn unary_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/unary_expr",
        common::render("def f\n  -x\n  ~x\nend\n")
    );
}

#[test]
fn postfix_ops() {
    loom_snapshot::assert_snapshot!(
        "expressions/postfix_ops",
        common::render("def f\n  x.f(1)\n  x.0\n  x { 1 }\n  x do 1 end\n  a[0, 1]\n  res?\nend\n")
    );
}

#[test]
fn call_args_shorthand() {
    loom_snapshot::assert_snapshot!(
        "expressions/call_args_shorthand",
        common::render("def f\n  g(a, b:)\nend\n")
    );
}

#[test]
fn primary_literals() {
    loom_snapshot::assert_snapshot!(
        "expressions/primary_literals",
        common::render(
            "def f\n  123\n  1.5\n  'a'\n  \"hello\"\n  r#\"raw\"#\n  b\"bytes\"\n  true\n  false\nend\n"
        )
    );
}

#[test]
fn primary_at_ident() {
    loom_snapshot::assert_snapshot!(
        "expressions/primary_at_ident",
        common::render("def f\n  @field\nend\n")
    );
}

#[test]
fn tuple_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/tuple_expr",
        common::render("def f\n  ()\n  (1,)\n  (1, 2)\nend\n")
    );
}

#[test]
fn array_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/array_expr",
        common::render("def f\n  []\n  [1, 2]\nend\n")
    );
}

#[test]
fn map_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/map_expr",
        common::render("def f\n  {}\n  {a => b, c => d}\nend\n")
    );
}

#[test]
fn closure_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/closure_expr",
        common::render("def f\n  x = {|a, mut b: Int| a + b}\nend\n")
    );
}

#[test]
fn if_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/if_expr",
        common::render("def f\n  if c1 then s1\n  elsif c2\n    s2\n  else\n    s3\n  end\nend\n")
    );
}

#[test]
fn case_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/case_expr",
        common::render(
            "def f\n  case x\n  in 1 then s1\n  in 2 if cond\n    s2\n  else\n    s3\n  end\nend\n"
        )
    );
}

#[test]
fn loops() {
    loom_snapshot::assert_snapshot!(
        "expressions/loops",
        common::render(
            "def f\n  loop do break end\n  while cond\n    next\n  end\n  for x in xs\n    return x\n  end\nend\n"
        )
    );
}

#[test]
fn select_expr() {
    loom_snapshot::assert_snapshot!(
        "expressions/select_expr",
        common::render(
            "def f\n  select\n  when x = rx then s1\n  when ry\n    s2\n  else\n    s3\n  end\nend\n"
        )
    );
}

#[test]
fn unsafe_block() {
    loom_snapshot::assert_snapshot!(
        "expressions/unsafe_block",
        common::render("def f\n  unsafe do\n    s1\n  end\nend\n")
    );
}

#[test]
fn leading_dot_chain() {
    loom_snapshot::assert_snapshot!(
        "expressions/leading_dot_chain",
        common::render("def f\n  x\n    # comment\n    .f()\n    .g()\nend\n")
    );
}

#[test]
fn trailing_block_brace() {
    loom_snapshot::assert_snapshot!(
        "expressions/trailing_block_brace",
        common::render("def f\n  f(1) { |x| x }\nend\n")
    );
}

#[test]
fn trailing_block_do() {
    loom_snapshot::assert_snapshot!(
        "expressions/trailing_block_do",
        common::render("def f\n  f(1) do |x|\n    x\n  end\nend\n")
    );
}

#[test]
fn control_flow_jump() {
    loom_snapshot::assert_snapshot!(
        "expressions/control_flow_jump",
        common::render("def f\n  return 1\n  break 2\n  next\nend\n")
    );
}
