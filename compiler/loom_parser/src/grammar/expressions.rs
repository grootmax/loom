//! Statements, blocks and expressions (spec §4, §7).

use loom_diagnostics::{Diagnostic, codes};
use loom_span::TextRange;
use loom_syntax::SyntaxKind::{
    self, AMP, AND_KW, ARG, ARG_LIST, ARRAY_EXPR, ASSIGN_STMT, AT_IDENT, BINARY_EXPR, BIND_STMT,
    BLOCK, BLOCK_PARAM, BLOCK_PARAM_LIST, BREAK_EXPR, BREAK_KW, BYTE_STRING_LIT, CALL_EXPR, CARET,
    CASE_ARM, CASE_EXPR, CASE_KW, CHAR_LIT, CLOSURE_EXPR, COLON, COMMA, DO_KW, DOT, DOT2, DOT2_EQ,
    ELSE_BRANCH, ELSE_KW, ELSIF_BRANCH, ELSIF_KW, END_KW, EOF, EQ, EQ2, ERROR, EXPR_STMT, FALSE_KW,
    FAT_ARROW, FIELD_EXPR, FIELD_REF_EXPR, FLOAT_LIT, FOR_EXPR, FOR_KW, GE, GT, GUARD, IDENT,
    IF_EXPR, IF_KW, IN_EXPR, IN_KW, INDEX_EXPR, INT_LIT, INTERP_END, INTERP_START, INTERPOLATION,
    L_BRACE, L_BRACKET, L_PAREN, LE, LITERAL, LOOP_EXPR, LOOP_KW, LT, MAP_ENTRY, MAP_EXPR,
    METHOD_CALL_EXPR, MINUS, MUT_KW, NAME_REF, NAMED_ARG, NEQ, NEWLINE, NEXT_EXPR, NEXT_KW, NOT_KW,
    OR_KW, OWN_KW, PACKAGE_KW, PAREN_EXPR, PATH_EXPR, PERCENT, PIPE, PLUS, PREFIX_EXPR, QUESTION,
    R_BRACE, R_BRACKET, R_PAREN, RANGE_EXPR, RAW_STRING_LIT, RETURN_EXPR, RETURN_KW, SELECT_ARM,
    SELECT_EXPR, SELECT_KW, SELF_KW, SELF_TYPE_KW, SHL, SHR, SLASH, STAR, STRING, STRING_END,
    STRING_FRAGMENT, STRING_START, SUPER_KW, THEN_KW, TILDE, TRAILING_BLOCK, TRUE_KW, TRY_EXPR,
    TUPLE_EXPR, UNSAFE_EXPR, UNSAFE_KW, WHEN_KW, WHILE_EXPR, WHILE_KW,
};

use super::{BLOCK_TERMINATORS, at_column_one_item, end_of_line, expect_end, patterns, types};
use crate::parser::{CompletedMarker, Parser};

// ----- blocks and statements (§4) ----------------------------------------------------------

/// `block_body`: statements separated by line breaks, up to a block terminator. Produces
/// a `BLOCK` node.
pub(crate) fn block_body(p: &mut Parser<'_>) {
    let block = p.start();
    p.push_newlines(true);
    if p.enter() {
        loop {
            p.eat_newlines();
            if p.at_any(BLOCK_TERMINATORS) || at_column_one_item(p) {
                break;
            }
            if p.at(R_PAREN) || p.at(R_BRACKET) {
                let range = p.current_range();
                p.error_at(
                    codes::UNEXPECTED_CLOSING,
                    range,
                    format!("unexpected {}", p.current().describe()),
                );
                p.bump_error();
                continue;
            }
            statement(p);
            end_of_line(p, BLOCK_TERMINATORS);
        }
        p.leave();
    }
    p.pop_newlines();
    block.complete(p, BLOCK);
}

/// Whether the current token can start a statement.
pub(super) fn at_statement_start(p: &Parser<'_>) -> bool {
    p.at(SyntaxKind::VAR_KW) || at_expression_start(p)
}

fn statement(p: &mut Parser<'_>) {
    let marker = p.start();
    // `var x …`, `x: T = …` and `x = …` are bindings (spec §4.1).
    if p.at(SyntaxKind::VAR_KW) {
        p.bump();
        binding_rest(p);
        marker.complete(p, BIND_STMT);
        return;
    }
    if p.at(IDENT) && (p.nth(1) == COLON || p.nth(1) == EQ) {
        binding_rest(p);
        marker.complete(p, BIND_STMT);
        return;
    }
    if at_command_call(p) {
        command_call(p);
        marker.complete(p, EXPR_STMT);
        return;
    }
    let Some(lhs) = expr_bp(p, 0, true) else {
        // Not a statement: report once and skip the rest of the line.
        p.error_expected("a statement");
        while !p.at(NEWLINE) && !p.at(EOF) && !p.at_any(BLOCK_TERMINATORS) {
            p.bump();
        }
        marker.complete(p, ERROR);
        return;
    };
    if p.current().is_assignment_operator() {
        p.bump();
        expr(p);
        let _ = lhs;
        marker.complete(p, ASSIGN_STMT);
    } else {
        marker.complete(p, EXPR_STMT);
    }
}

/// `IDENT [":" type] "=" expr` (after an optional `var`).
fn binding_rest(p: &mut Parser<'_>) {
    super::items::name(p);
    if p.eat(COLON) {
        types::type_(p);
    }
    if p.current().is_assignment_operator() && !p.at(EQ) {
        p.error_expected("`=`");
        p.bump();
    } else {
        p.expect(EQ);
    }
    expr(p);
}

/// A command call (spec §4.4): `IDENT` + whitespace + a token that can start an argument.
fn at_command_call(p: &Parser<'_>) -> bool {
    if !p.at(IDENT) || !p.space_after(0) {
        return false;
    }
    matches!(
        p.nth(1),
        INT_LIT
            | FLOAT_LIT
            | CHAR_LIT
            | STRING_START
            | RAW_STRING_LIT
            | BYTE_STRING_LIT
            | TRUE_KW
            | FALSE_KW
            | IDENT
            | AT_IDENT
            | SELF_KW
            | SELF_TYPE_KW
            | NOT_KW
    )
}

fn command_call(p: &mut Parser<'_>) {
    let call = p.start();
    let callee = p.start();
    let name = p.start();
    p.bump();
    name.complete(p, NAME_REF);
    callee.complete(p, PATH_EXPR);
    let args = p.start();
    let mut seen_named = false;
    loop {
        argument(p, &mut seen_named);
        if !p.eat(COMMA) {
            break;
        }
    }
    args.complete(p, ARG_LIST);
    if at_trailing_block(p) {
        trailing_block(p);
    }
    call.complete(p, CALL_EXPR);
}

// ----- expressions (§7) ------------------------------------------------------------------

/// Whether the current token can start an expression.
pub(super) fn at_expression_start(p: &Parser<'_>) -> bool {
    p.at_any(&[
        INT_LIT,
        FLOAT_LIT,
        CHAR_LIT,
        STRING_START,
        RAW_STRING_LIT,
        BYTE_STRING_LIT,
        TRUE_KW,
        FALSE_KW,
        IDENT,
        AT_IDENT,
        SELF_KW,
        SELF_TYPE_KW,
        SUPER_KW,
        PACKAGE_KW,
        L_PAREN,
        L_BRACKET,
        L_BRACE,
        IF_KW,
        CASE_KW,
        LOOP_KW,
        WHILE_KW,
        FOR_KW,
        SELECT_KW,
        UNSAFE_KW,
        RETURN_KW,
        BREAK_KW,
        NEXT_KW,
        MINUS,
        TILDE,
        NOT_KW,
    ])
}

/// Parses an expression; reports E0100 if there is none.
pub(crate) fn expr(p: &mut Parser<'_>) {
    if expr_bp(p, 0, false).is_none() {
        recover_expression(p);
    }
}

/// Reports a missing expression and skips one token unless it ends a construct.
fn recover_expression(p: &mut Parser<'_>) {
    p.error_expected("an expression");
    if !p.at_any(&[
        NEWLINE, EOF, END_KW, R_PAREN, R_BRACKET, R_BRACE, INTERP_END, STRING_END, THEN_KW, DO_KW,
        COMMA, ELSE_KW, ELSIF_KW, IN_KW, WHEN_KW,
    ]) {
        p.bump_error();
    }
}

/// Binding power and node kind of an infix operator. Higher binds tighter (spec §7.5).
fn infix(kind: SyntaxKind) -> Option<(u8, SyntaxKind)> {
    Some(match kind {
        OR_KW => (1, BINARY_EXPR),
        AND_KW => (2, BINARY_EXPR),
        EQ2 | NEQ | LT | LE | GT | GE => (4, BINARY_EXPR),
        IN_KW => (4, IN_EXPR),
        DOT2 | DOT2_EQ => (5, RANGE_EXPR),
        PIPE => (6, BINARY_EXPR),
        CARET => (7, BINARY_EXPR),
        AMP => (8, BINARY_EXPR),
        SHL | SHR => (9, BINARY_EXPR),
        PLUS | MINUS => (10, BINARY_EXPR),
        STAR | SLASH | PERCENT => (11, BINARY_EXPR),
        _ => return None,
    })
}

/// `not` binds looser than comparison: its operand is a comparison-level expression.
const NOT_OPERAND_POWER: u8 = 4;
/// Unary `-` and `~` bind tighter than any binary operator.
const UNARY_OPERAND_POWER: u8 = 12;

/// Pratt loop. `statement_level` lets an assignment operator end the expression so the
/// statement parser can handle it; elsewhere an assignment is E0106.
fn expr_bp(p: &mut Parser<'_>, min_power: u8, statement_level: bool) -> Option<CompletedMarker> {
    if !p.enter() {
        return None;
    }
    let result = expr_bp_inner(p, min_power, statement_level);
    p.leave();
    result
}

fn expr_bp_inner(
    p: &mut Parser<'_>,
    min_power: u8,
    statement_level: bool,
) -> Option<CompletedMarker> {
    let mut lhs = match p.current() {
        NOT_KW => {
            let marker = p.start();
            p.bump();
            if expr_bp(p, NOT_OPERAND_POWER, false).is_none() {
                recover_expression(p);
            }
            marker.complete(p, PREFIX_EXPR)
        }
        MINUS | TILDE => {
            let marker = p.start();
            p.bump();
            if expr_bp(p, UNARY_OPERAND_POWER, false).is_none() {
                recover_expression(p);
            }
            marker.complete(p, PREFIX_EXPR)
        }
        _ => {
            let primary = primary(p)?;
            postfix(p, primary)
        }
    };
    // The last non-associative level seen, to report `a < b < c` and `a..b..c`.
    let mut previous_level: Option<u8> = None;
    loop {
        let operator = p.current();
        if operator.is_assignment_operator() {
            if statement_level && min_power == 0 {
                break;
            }
            let range = p.current_range();
            let diagnostic = Diagnostic::error(
                codes::ASSIGNMENT_IN_EXPRESSION,
                p.span(range),
                "assignment is only allowed as a statement",
            )
            .with_label("not allowed here")
            .with_help("use `==` to compare");
            p.push_diagnostic(diagnostic);
            let marker = lhs.precede(p);
            p.bump();
            expr(p);
            lhs = marker.complete(p, ASSIGN_STMT);
            continue;
        }
        let Some((power, kind)) = infix(operator) else {
            break;
        };
        // Left-associative: the right operand is parsed with `power + 1`, so an operator
        // of the same level ends it. Chains of the non-associative levels are reported
        // through `previous_level` below.
        if power < min_power {
            break;
        }
        if power == 4 || power == 5 {
            if previous_level == Some(power) {
                let range = p.current_range();
                let (code, message) = if power == 4 {
                    (
                        codes::CHAINED_COMPARISON,
                        "comparison operators cannot be chained",
                    )
                } else {
                    (codes::CHAINED_RANGE, "range operators cannot be chained")
                };
                let diagnostic = Diagnostic::error(code, p.span(range), message)
                    .with_label("second operator")
                    .with_help(if power == 4 {
                        "split it with `and`: `a < b and b < c`"
                    } else {
                        "add parentheses to say which range you mean"
                    });
                p.push_diagnostic(diagnostic);
            }
            previous_level = Some(power);
        }
        let marker = lhs.precede(p);
        p.bump();
        if kind == IN_EXPR {
            patterns::pattern(p);
        } else if expr_bp(p, power + 1, false).is_none() {
            recover_expression(p);
        }
        lhs = marker.complete(p, kind);
    }
    Some(lhs)
}

fn primary(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    let marker = match p.current() {
        INT_LIT | FLOAT_LIT | CHAR_LIT | RAW_STRING_LIT | BYTE_STRING_LIT | TRUE_KW | FALSE_KW => {
            let marker = p.start();
            p.bump();
            marker.complete(p, LITERAL)
        }
        STRING_START => string(p),
        IDENT | SELF_KW | SELF_TYPE_KW | SUPER_KW | PACKAGE_KW => {
            let marker = p.start();
            let name = p.start();
            p.bump();
            name.complete(p, NAME_REF);
            marker.complete(p, PATH_EXPR)
        }
        AT_IDENT => {
            let marker = p.start();
            p.bump();
            marker.complete(p, FIELD_REF_EXPR)
        }
        L_PAREN => paren_or_tuple(p),
        L_BRACKET => array(p),
        L_BRACE => {
            if p.nth(1) == PIPE {
                block(p, CLOSURE_EXPR)
            } else {
                map(p)
            }
        }
        IF_KW => if_expr(p),
        CASE_KW => case_expr(p),
        LOOP_KW => keyword_block(p, LOOP_EXPR),
        UNSAFE_KW => keyword_block(p, UNSAFE_EXPR),
        WHILE_KW => while_expr(p),
        FOR_KW => for_expr(p),
        SELECT_KW => select_expr(p),
        RETURN_KW | BREAK_KW => {
            let kind = if p.at(RETURN_KW) {
                RETURN_EXPR
            } else {
                BREAK_EXPR
            };
            let marker = p.start();
            p.bump();
            if at_expression_start(p) {
                expr(p);
            }
            marker.complete(p, kind)
        }
        NEXT_KW => {
            let marker = p.start();
            p.bump();
            marker.complete(p, NEXT_EXPR)
        }
        _ => return None,
    };
    Some(marker)
}

/// A `STRING` node: `STRING_START { STRING_FRAGMENT | INTERPOLATION } STRING_END`.
pub(super) fn string(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    p.bump(); // STRING_START
    loop {
        match p.nth_raw(0) {
            STRING_FRAGMENT => p.bump(),
            INTERP_START => interpolation(p),
            STRING_END => {
                p.bump();
                break;
            }
            // Unterminated: the lexer already reported E0018.
            _ => break,
        }
    }
    marker.complete(p, STRING)
}

fn interpolation(p: &mut Parser<'_>) {
    let marker = p.start();
    let opener = p.current_range();
    p.bump(); // #{
    p.push_newlines(false);
    if !p.at(INTERP_END) {
        expr(p);
    }
    // Anything else before the closing `}` is an error.
    if !p.at(INTERP_END) && !p.at(EOF) {
        p.error_expected("`}`");
        let error = p.start();
        while !p.at(INTERP_END) && !p.at(EOF) && !p.at(STRING_END) {
            p.bump();
        }
        error.complete(p, ERROR);
    }
    p.pop_newlines();
    if !p.eat(INTERP_END) && !p.at(EOF) {
        unclosed(p, opener);
    }
    marker.complete(p, INTERPOLATION);
}

fn paren_or_tuple(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    let mut count = 0;
    let mut trailing_comma = false;
    while !p.at(R_PAREN) && !p.at(EOF) {
        expr(p);
        count += 1;
        trailing_comma = p.eat(COMMA);
        if !trailing_comma {
            break;
        }
    }
    p.pop_newlines();
    expect_closing(p, R_PAREN, opener);
    let kind = if count == 1 && !trailing_comma {
        PAREN_EXPR
    } else {
        TUPLE_EXPR
    };
    marker.complete(p, kind)
}

fn array(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_BRACKET) && !p.at(EOF) {
        expr(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expect_closing(p, R_BRACKET, opener);
    marker.complete(p, ARRAY_EXPR)
}

fn map(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_BRACE) && !p.at(EOF) {
        let entry = p.start();
        expr(p);
        if p.expect(FAT_ARROW) {
            expr(p);
        }
        entry.complete(p, MAP_ENTRY);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expect_closing(p, R_BRACE, opener);
    marker.complete(p, MAP_EXPR)
}

/// Expects the closing delimiter of a bracketed construct opened at `opener`. A missing
/// closer is E0101 at the opener (spec Appendix C.2). Unexpected tokens before it are
/// reported once and skipped.
pub(super) fn expect_closing(p: &mut Parser<'_>, closer: SyntaxKind, opener: TextRange) {
    // Line breaks before the closer belong to the bracketed context.
    p.push_newlines(false);
    if p.eat(closer) {
        p.pop_newlines();
        return;
    }
    if p.at(EOF) || at_column_one_item(p) {
        p.pop_newlines();
        unclosed(p, opener);
        return;
    }
    p.error_expected(&format!("`,` or {}", closer.describe()));
    let error = p.start();
    let mut balance = 0usize;
    while !p.at(EOF) && !at_column_one_item(p) {
        match p.current() {
            kind if kind == closer && balance == 0 => break,
            L_PAREN | L_BRACKET | L_BRACE => balance += 1,
            R_PAREN | R_BRACKET | R_BRACE => balance = balance.saturating_sub(1),
            _ => {}
        }
        p.bump();
    }
    error.complete(p, ERROR);
    p.pop_newlines();
    p.eat(closer);
}

fn unclosed(p: &mut Parser<'_>, opener: TextRange) {
    let text = p.text[opener.to_usize()].to_owned();
    let diagnostic = Diagnostic::error(
        codes::UNCLOSED_DELIMITER,
        p.span(opener),
        format!("unclosed delimiter `{text}`"),
    )
    .with_label("never closed");
    p.push_diagnostic(diagnostic);
}

// ----- postfix: calls, fields, indexing, `?`, trailing blocks (§7.3, §7.4) -----------------

fn postfix(p: &mut Parser<'_>, mut lhs: CompletedMarker) -> CompletedMarker {
    loop {
        lhs = match p.current() {
            DOT => {
                let marker = lhs.precede(p);
                p.bump();
                match p.current() {
                    INT_LIT => {
                        p.bump();
                        marker.complete(p, FIELD_EXPR)
                    }
                    IDENT => {
                        let name = p.start();
                        p.bump();
                        name.complete(p, NAME_REF);
                        let mut is_call = false;
                        if p.at(L_PAREN) {
                            argument_list(p);
                            is_call = true;
                        }
                        if at_trailing_block(p) {
                            trailing_block(p);
                            is_call = true;
                        }
                        marker.complete(
                            p,
                            if is_call {
                                METHOD_CALL_EXPR
                            } else {
                                FIELD_EXPR
                            },
                        )
                    }
                    _ => {
                        p.error_expected("a field or method name");
                        marker.complete(p, FIELD_EXPR)
                    }
                }
            }
            L_PAREN => {
                let marker = lhs.precede(p);
                argument_list(p);
                if at_trailing_block(p) {
                    trailing_block(p);
                }
                marker.complete(p, CALL_EXPR)
            }
            L_BRACKET => {
                let marker = lhs.precede(p);
                let opener = p.current_range();
                p.bump();
                p.push_newlines(false);
                while !p.at(R_BRACKET) && !p.at(EOF) {
                    expr(p);
                    if !p.eat(COMMA) {
                        break;
                    }
                }
                p.pop_newlines();
                expect_closing(p, R_BRACKET, opener);
                marker.complete(p, INDEX_EXPR)
            }
            QUESTION => {
                let marker = lhs.precede(p);
                p.bump();
                marker.complete(p, TRY_EXPR)
            }
            L_BRACE | DO_KW if lhs.kind() == PATH_EXPR && at_trailing_block(p) => {
                let marker = lhs.precede(p);
                trailing_block(p);
                marker.complete(p, CALL_EXPR)
            }
            _ => return lhs,
        };
    }
}

/// Whether a trailing block follows on the same line (spec §7.3).
fn at_trailing_block(p: &Parser<'_>) -> bool {
    // In significant-newline contexts a line break shows up as `NEWLINE`, so `{`/`do` as
    // the current token is necessarily on the same line.
    p.nth_raw(0) == L_BRACE || p.nth_raw(0) == DO_KW
}

fn argument_list(p: &mut Parser<'_>) {
    let list = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    let mut seen_named = false;
    while !p.at(R_PAREN) && !p.at(EOF) {
        argument(p, &mut seen_named);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expect_closing(p, R_PAREN, opener);
    list.complete(p, ARG_LIST);
}

/// `expr` or `name: [expr]`.
fn argument(p: &mut Parser<'_>, seen_named: &mut bool) {
    let marker = p.start();
    if p.at(IDENT) && p.nth(1) == COLON {
        let name = p.start();
        p.bump();
        name.complete(p, NAME_REF);
        p.bump(); // :
        if !p.at(COMMA) && !p.at(R_PAREN) && !p.at(NEWLINE) && !p.at(EOF) {
            expr(p);
        }
        *seen_named = true;
        marker.complete(p, NAMED_ARG);
        return;
    }
    if *seen_named {
        let range = p.current_range();
        let diagnostic = Diagnostic::error(
            codes::POSITIONAL_AFTER_NAMED,
            p.span(range),
            "positional argument after a named argument",
        )
        .with_label("positional argument")
        .with_help("pass positional arguments first, or name this one too");
        p.push_diagnostic(diagnostic);
    }
    expr(p);
    marker.complete(p, ARG);
}

fn trailing_block(p: &mut Parser<'_>) {
    block(p, TRAILING_BLOCK);
}

/// `"{" [params] block_body "}"` or `"do" [params] block_body "end"`.
fn block(p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    let braced = p.at(L_BRACE);
    p.bump();
    p.push_newlines(true);
    if p.at(PIPE) {
        block_params(p);
    } else if kind == CLOSURE_EXPR {
        p.error_expected("`|` to start the closure's parameters");
    }
    block_body(p);
    p.pop_newlines();
    if braced {
        if !p.eat(R_BRACE) {
            unclosed(p, opener);
        }
    } else {
        expect_end(p, opener);
    }
    marker.complete(p, kind)
}

fn block_params(p: &mut Parser<'_>) {
    let list = p.start();
    p.bump(); // |
    p.push_newlines(false);
    while !p.at(PIPE) && !p.at(EOF) {
        let param = p.start();
        if p.at(MUT_KW) || p.at(OWN_KW) {
            p.bump();
        }
        patterns::pattern_atom(p);
        if p.eat(COLON) {
            types::type_(p);
        }
        param.complete(p, BLOCK_PARAM);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    p.expect(PIPE);
    list.complete(p, BLOCK_PARAM_LIST);
}

// ----- control flow (§7.6–§7.8, §11.3, §13) ------------------------------------------------

/// `then` + statements, or a line break + statements, as a `BLOCK`.
fn branch_body(p: &mut Parser<'_>) {
    if p.eat(THEN_KW) || p.at(NEWLINE) {
        block_body(p);
    } else {
        p.error_expected("`then` or a line break");
        block_body(p);
    }
}

fn condition(p: &mut Parser<'_>) {
    p.push_newlines(true);
    expr(p);
    p.pop_newlines();
}

fn if_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    condition(p);
    branch_body(p);
    loop {
        if p.at(ELSIF_KW) {
            let branch = p.start();
            p.bump();
            condition(p);
            branch_body(p);
            branch.complete(p, ELSIF_BRANCH);
        } else if p.at(ELSE_KW) && p.nth(1) == IF_KW {
            // `else if` (Ruby, Rust) is recovered as `elsif` (spec Appendix C.2, E0110).
            let branch = p.start();
            p.bump();
            let range = p.current_range();
            let diagnostic = Diagnostic::error(
                codes::ELSE_IF,
                p.span(range),
                "`else if` is written `elsif`",
            )
            .with_label("use `elsif`");
            p.push_diagnostic(diagnostic);
            p.bump();
            condition(p);
            branch_body(p);
            branch.complete(p, ELSIF_BRANCH);
        } else {
            break;
        }
    }
    if p.at(ELSE_KW) {
        let branch = p.start();
        p.bump();
        block_body(p);
        branch.complete(p, ELSE_BRANCH);
    }
    expect_end(p, opener);
    marker.complete(p, IF_EXPR)
}

fn case_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    condition(p);
    if !p.at(NEWLINE) {
        p.error_expected("a line break");
    }
    p.push_newlines(true);
    loop {
        p.eat_newlines();
        if !p.at(IN_KW) {
            break;
        }
        let arm = p.start();
        p.bump();
        patterns::pattern(p);
        if p.at(IF_KW) {
            let guard = p.start();
            p.bump();
            expr(p);
            guard.complete(p, GUARD);
        }
        branch_body(p);
        arm.complete(p, CASE_ARM);
    }
    if p.at(ELSE_KW) {
        let branch = p.start();
        p.bump();
        block_body(p);
        branch.complete(p, ELSE_BRANCH);
    }
    p.pop_newlines();
    expect_end(p, opener);
    marker.complete(p, CASE_EXPR)
}

/// `loop do … end` and `unsafe do … end`.
fn keyword_block(p: &mut Parser<'_>, kind: SyntaxKind) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.expect(DO_KW);
    block_body(p);
    expect_end(p, opener);
    marker.complete(p, kind)
}

fn while_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    condition(p);
    if !p.at(NEWLINE) {
        p.error_expected("a line break");
    }
    block_body(p);
    expect_end(p, opener);
    marker.complete(p, WHILE_EXPR)
}

fn for_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    patterns::pattern(p);
    if p.expect(IN_KW) {
        condition(p);
    }
    if !p.at(NEWLINE) {
        p.error_expected("a line break");
    }
    block_body(p);
    expect_end(p, opener);
    marker.complete(p, FOR_EXPR)
}

fn select_expr(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(true);
    loop {
        p.eat_newlines();
        if !p.at(WHEN_KW) {
            break;
        }
        let arm = p.start();
        p.bump();
        if select_arm_has_pattern(p) {
            patterns::pattern(p);
            p.expect(EQ);
        }
        condition(p);
        branch_body(p);
        arm.complete(p, SELECT_ARM);
    }
    if p.at(ELSE_KW) {
        let branch = p.start();
        p.bump();
        block_body(p);
        branch.complete(p, ELSE_BRANCH);
    }
    p.pop_newlines();
    expect_end(p, opener);
    marker.complete(p, SELECT_EXPR)
}

/// Whether a `=` appears at bracket depth 0 before `then` or the end of the line
/// (spec §11.3).
fn select_arm_has_pattern(p: &Parser<'_>) -> bool {
    let mut depth = 0usize;
    for n in 0.. {
        match p.nth_raw(n) {
            L_PAREN | L_BRACKET | L_BRACE => depth += 1,
            R_PAREN | R_BRACKET | R_BRACE => depth = depth.saturating_sub(1),
            EQ if depth == 0 => return true,
            THEN_KW | EOF => return false,
            NEWLINE if depth == 0 => return false,
            _ => {}
        }
    }
    false
}
