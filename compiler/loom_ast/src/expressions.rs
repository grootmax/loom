//! Statements, blocks and expressions (spec §4, §7).

use loom_syntax::{SyntaxKind, SyntaxToken};

use crate::{
    AstNode, Name, NameRef, Pattern, Type, ast_enum, ast_node, child, children,
    first_significant_token, token,
};

ast_node!(
    /// A sequence of statements: a function body, a branch, a closure body.
    Block, BLOCK
);

impl Block {
    /// The statements, in order.
    pub fn statements(&self) -> impl Iterator<Item = Stmt> + use<> {
        children(&self.0)
    }

    /// The final expression statement, whose value is the block's value.
    #[must_use]
    pub fn tail_expr(&self) -> Option<Expr> {
        match self.statements().last()? {
            Stmt::ExprStmt(statement) => statement.expr(),
            _ => None,
        }
    }
}

ast_enum!(
    /// A statement.
    Stmt { BindStmt, AssignStmt, ExprStmt }
);

ast_node!(
    /// `[var] name [: Type] = expr`. A plain `x = expr` may turn out to be an assignment
    /// to an existing `var` (spec §4.1); name resolution decides.
    BindStmt, BIND_STMT
);

impl BindStmt {
    /// Whether the binding is declared with `var`.
    #[must_use]
    pub fn is_var(&self) -> bool {
        token(&self.0, SyntaxKind::VAR_KW).is_some()
    }

    /// The bound name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The declared type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `place op= expr`.
    AssignStmt, ASSIGN_STMT
);

impl AssignStmt {
    /// The place being assigned.
    #[must_use]
    pub fn place(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// The operator: `=` or a compound assignment.
    #[must_use]
    pub fn operator(&self) -> Option<SyntaxToken> {
        self.0
            .children_with_tokens()
            .filter_map(loom_syntax::SyntaxElement::into_token)
            .find(|t| t.kind().is_assignment_operator())
    }

    /// The assigned value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

ast_node!(
    /// An expression used as a statement.
    ExprStmt, EXPR_STMT
);

impl ExprStmt {
    /// The expression.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_enum!(
    /// Any expression.
    Expr {
        Literal, StringLit, PathExpr, FieldRefExpr, ParenExpr, TupleExpr, ArrayExpr, MapExpr,
        ClosureExpr, IfExpr, CaseExpr, LoopExpr, WhileExpr, ForExpr, SelectExpr, UnsafeExpr,
        ReturnExpr, BreakExpr, NextExpr, BinaryExpr, PrefixExpr, RangeExpr, InExpr, CallExpr,
        MethodCallExpr, FieldExpr, IndexExpr, TryExpr, AssignStmt,
    }
);

ast_node!(
    /// A literal other than a (possibly interpolated) string.
    Literal, LITERAL
);

impl Literal {
    /// The literal token.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_significant_token(&self.0)
    }
}

ast_node!(
    /// A string literal, possibly with interpolations.
    StringLit, STRING
);

impl StringLit {
    /// Whether this is a triple-quoted string.
    #[must_use]
    pub fn is_triple_quoted(&self) -> bool {
        token(&self.0, SyntaxKind::STRING_START).is_some_and(|t| t.text() == "\"\"\"")
    }

    /// The text fragments (raw, with escapes) and interpolations, in order.
    pub fn parts(&self) -> impl Iterator<Item = StringPart> + use<> {
        self.0
            .children_with_tokens()
            .filter_map(|element| match element {
                loom_syntax::SyntaxElement::Token(t) if t.kind() == SyntaxKind::STRING_FRAGMENT => {
                    Some(StringPart::Text(t))
                }
                loom_syntax::SyntaxElement::Node(n) => {
                    Interpolation::cast(n).map(StringPart::Interpolation)
                }
                loom_syntax::SyntaxElement::Token(_) => None,
            })
    }
}

/// A part of a string literal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StringPart {
    /// Raw text, escapes not yet processed.
    Text(SyntaxToken),
    /// `#{expr}`.
    Interpolation(Interpolation),
}

ast_node!(
    /// `#{expr}` inside a string.
    Interpolation, INTERPOLATION
);

impl Interpolation {
    /// The embedded expression.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// A name in expression position: a variable, function, type, module or `self`.
    PathExpr, PATH_EXPR
);

impl PathExpr {
    /// The name.
    #[must_use]
    pub fn name_ref(&self) -> Option<NameRef> {
        child(&self.0)
    }
}

ast_node!(
    /// `@field`, meaning `self.field` (spec §7.4).
    FieldRefExpr, FIELD_REF_EXPR
);

impl FieldRefExpr {
    /// The field name, without the `@`.
    #[must_use]
    pub fn field_name(&self) -> String {
        token(&self.0, SyntaxKind::AT_IDENT)
            .map(|t| t.text().trim_start_matches('@').to_owned())
            .unwrap_or_default()
    }
}

ast_node!(
    /// `(expr)`.
    ParenExpr, PAREN_EXPR
);

impl ParenExpr {
    /// The inner expression.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `()` or `(a, b)`.
    TupleExpr, TUPLE_EXPR
);

impl TupleExpr {
    /// The elements.
    pub fn elements(&self) -> impl Iterator<Item = Expr> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `[a, b]`.
    ArrayExpr, ARRAY_EXPR
);

impl ArrayExpr {
    /// The elements.
    pub fn elements(&self) -> impl Iterator<Item = Expr> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `{k => v, …}`.
    MapExpr, MAP_EXPR
);

impl MapExpr {
    /// The entries.
    pub fn entries(&self) -> impl Iterator<Item = MapEntry> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `key => value`.
    MapEntry, MAP_ENTRY
);

impl MapEntry {
    /// The key.
    #[must_use]
    pub fn key(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

ast_node!(
    /// A standalone closure: `{ |params| body }`.
    ClosureExpr, CLOSURE_EXPR
);

impl ClosureExpr {
    /// The parameters.
    #[must_use]
    pub fn params(&self) -> Option<BlockParamList> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// A block passed after a call: `{ |x| … }` or `do |x| … end`.
    TrailingBlock, TRAILING_BLOCK
);

impl TrailingBlock {
    /// Whether this is the `do … end` form.
    #[must_use]
    pub fn is_do(&self) -> bool {
        token(&self.0, SyntaxKind::DO_KW).is_some()
    }

    /// The parameters, if any.
    #[must_use]
    pub fn params(&self) -> Option<BlockParamList> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `|a, mut b: T|`.
    BlockParamList, BLOCK_PARAM_LIST
);

impl BlockParamList {
    /// The parameters.
    pub fn params(&self) -> impl Iterator<Item = BlockParam> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `[mut | own] pattern [: Type]`.
    BlockParam, BLOCK_PARAM
);

impl BlockParam {
    /// The pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }

    /// The declared type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }
}

ast_node!(
    /// `if … elsif … else … end`.
    IfExpr, IF_EXPR
);

impl IfExpr {
    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The first branch.
    #[must_use]
    pub fn then_branch(&self) -> Option<Block> {
        child(&self.0)
    }

    /// The `elsif` branches.
    pub fn elsif_branches(&self) -> impl Iterator<Item = ElsifBranch> + use<> {
        children(&self.0)
    }

    /// The `else` branch.
    #[must_use]
    pub fn else_branch(&self) -> Option<ElseBranch> {
        child(&self.0)
    }
}

ast_node!(
    /// `elsif condition …`.
    ElsifBranch, ELSIF_BRANCH
);

impl ElsifBranch {
    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `else …` in `if`, `case` or `select`.
    ElseBranch, ELSE_BRANCH
);

impl ElseBranch {
    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `case scrutinee … end`.
    CaseExpr, CASE_EXPR
);

impl CaseExpr {
    /// The value being matched.
    #[must_use]
    pub fn scrutinee(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The arms.
    pub fn arms(&self) -> impl Iterator<Item = CaseArm> + use<> {
        children(&self.0)
    }

    /// The `else` arm.
    #[must_use]
    pub fn else_branch(&self) -> Option<ElseBranch> {
        child(&self.0)
    }
}

ast_node!(
    /// `in pattern [if guard] then …`.
    CaseArm, CASE_ARM
);

impl CaseArm {
    /// The pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }

    /// The guard condition.
    #[must_use]
    pub fn guard(&self) -> Option<Expr> {
        self.0
            .child_of_kind(SyntaxKind::GUARD)
            .and_then(|guard| child(&guard))
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `loop do … end`.
    LoopExpr, LOOP_EXPR
);

impl LoopExpr {
    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `while condition … end`.
    WhileExpr, WHILE_EXPR
);

impl WhileExpr {
    /// The condition.
    #[must_use]
    pub fn condition(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `for pattern in iterable … end`.
    ForExpr, FOR_EXPR
);

impl ForExpr {
    /// The loop pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }

    /// The iterated expression.
    #[must_use]
    pub fn iterable(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `select … end`.
    SelectExpr, SELECT_EXPR
);

impl SelectExpr {
    /// The arms.
    pub fn arms(&self) -> impl Iterator<Item = SelectArm> + use<> {
        children(&self.0)
    }

    /// The `else` arm.
    #[must_use]
    pub fn else_branch(&self) -> Option<ElseBranch> {
        child(&self.0)
    }
}

ast_node!(
    /// `when [pattern =] expr then …`.
    SelectArm, SELECT_ARM
);

impl SelectArm {
    /// The receive pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }

    /// The awaited operation.
    #[must_use]
    pub fn operation(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The body.
    #[must_use]
    pub fn body(&self) -> Option<Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `unsafe do … end`.
    UnsafeExpr, UNSAFE_EXPR
);

ast_node!(
    /// `return [expr]`.
    ReturnExpr, RETURN_EXPR
);

impl ReturnExpr {
    /// The returned value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `break [expr]`.
    BreakExpr, BREAK_EXPR
);

impl BreakExpr {
    /// The loop's value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `next`.
    NextExpr, NEXT_EXPR
);

/// Shared accessors for operator expressions.
fn operator_token(node: &loom_syntax::SyntaxNode) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(loom_syntax::SyntaxElement::into_token)
        .find(|t| !t.kind().is_trivia() && t.kind() != SyntaxKind::NEWLINE)
}

ast_node!(
    /// `lhs op rhs` for arithmetic, bitwise, comparison and logical operators.
    BinaryExpr, BINARY_EXPR
);

impl BinaryExpr {
    /// The left operand.
    #[must_use]
    pub fn lhs(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// The operator.
    #[must_use]
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.0)
    }

    /// The right operand.
    #[must_use]
    pub fn rhs(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

ast_node!(
    /// `-x`, `~x` or `not x`.
    PrefixExpr, PREFIX_EXPR
);

impl PrefixExpr {
    /// The operator.
    #[must_use]
    pub fn operator(&self) -> Option<SyntaxToken> {
        operator_token(&self.0)
    }

    /// The operand.
    #[must_use]
    pub fn operand(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `a..b` or `a..=b`.
    RangeExpr, RANGE_EXPR
);

impl RangeExpr {
    /// The start.
    #[must_use]
    pub fn start(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// Whether the range is inclusive (`..=`).
    #[must_use]
    pub fn is_inclusive(&self) -> bool {
        token(&self.0, SyntaxKind::DOT2_EQ).is_some()
    }

    /// The end.
    #[must_use]
    pub fn end(&self) -> Option<Expr> {
        children(&self.0).nth(1)
    }
}

ast_node!(
    /// `expr in Pattern`, a pattern test (spec §7.6, §9).
    InExpr, IN_EXPR
);

impl InExpr {
    /// The tested value.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }
}

ast_node!(
    /// `callee(args) [block]`, `callee { block }`, or a command call `callee args`.
    CallExpr, CALL_EXPR
);

impl CallExpr {
    /// The called expression.
    #[must_use]
    pub fn callee(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The arguments.
    #[must_use]
    pub fn arg_list(&self) -> Option<ArgList> {
        child(&self.0)
    }

    /// The trailing block.
    #[must_use]
    pub fn trailing_block(&self) -> Option<TrailingBlock> {
        child(&self.0)
    }

    /// Whether this is a command call written without parentheses (spec §4.4).
    #[must_use]
    pub fn is_command(&self) -> bool {
        self.arg_list().is_some_and(|args| !args.is_parenthesized())
    }
}

ast_node!(
    /// `receiver.name(args) [block]` or `receiver.name block`.
    MethodCallExpr, METHOD_CALL_EXPR
);

impl MethodCallExpr {
    /// The receiver.
    #[must_use]
    pub fn receiver(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The method name.
    #[must_use]
    pub fn name_ref(&self) -> Option<NameRef> {
        child(&self.0)
    }

    /// The arguments, if parenthesized.
    #[must_use]
    pub fn arg_list(&self) -> Option<ArgList> {
        child(&self.0)
    }

    /// The trailing block.
    #[must_use]
    pub fn trailing_block(&self) -> Option<TrailingBlock> {
        child(&self.0)
    }
}

ast_node!(
    /// `receiver.name` (a field or a zero-argument method) or `tuple.0`.
    FieldExpr, FIELD_EXPR
);

impl FieldExpr {
    /// The receiver.
    #[must_use]
    pub fn receiver(&self) -> Option<Expr> {
        child(&self.0)
    }

    /// The field or method name.
    #[must_use]
    pub fn name_ref(&self) -> Option<NameRef> {
        child(&self.0)
    }

    /// The tuple index token, for `t.0`.
    #[must_use]
    pub fn index(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::INT_LIT)
    }
}

ast_node!(
    /// `base[args]`: indexing, or generic instantiation when `base` is a type (§7.2).
    IndexExpr, INDEX_EXPR
);

impl IndexExpr {
    /// The indexed expression.
    #[must_use]
    pub fn base(&self) -> Option<Expr> {
        children(&self.0).next()
    }

    /// The index expressions.
    pub fn indices(&self) -> impl Iterator<Item = Expr> + use<> {
        children(&self.0).skip(1)
    }
}

ast_node!(
    /// `expr?`.
    TryExpr, TRY_EXPR
);

impl TryExpr {
    /// The operand.
    #[must_use]
    pub fn expr(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// Call arguments, with or without parentheses.
    ArgList, ARG_LIST
);

impl ArgList {
    /// Whether the arguments are in parentheses.
    #[must_use]
    pub fn is_parenthesized(&self) -> bool {
        token(&self.0, SyntaxKind::L_PAREN).is_some()
    }

    /// The arguments, in order.
    pub fn args(&self) -> impl Iterator<Item = Argument> + use<> {
        children(&self.0)
    }
}

ast_enum!(
    /// A positional or named argument.
    Argument { Arg, NamedArg }
);

ast_node!(
    /// A positional argument.
    Arg, ARG
);

impl Arg {
    /// The value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `name: value` or the shorthand `name:`.
    NamedArg, NAMED_ARG
);

impl NamedArg {
    /// The parameter name.
    #[must_use]
    pub fn name_ref(&self) -> Option<NameRef> {
        child(&self.0)
    }

    /// The value; `None` for the shorthand.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}
