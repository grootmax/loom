//! The grammar of spec §3–§11, one function per production in `spec/grammar.ebnf`.

mod expressions;
mod items;
mod patterns;
mod types;

use loom_diagnostics::codes;
use loom_span::TextRange;
use loom_syntax::SyntaxKind::{
    self, DEF_KW, ELSE_KW, ELSIF_KW, END_KW, ENUM_KW, EOF, IMPL_KW, IN_KW, INTERP_END, MODULE_KW,
    NEWLINE, R_BRACE, R_BRACKET, R_PAREN, SOURCE_FILE, STRUCT_KW, TRAIT_KW, WHEN_KW,
};

use crate::parser::Parser;

/// Tokens that end a block body. Each construct checks that the one it needs is there.
const BLOCK_TERMINATORS: &[SyntaxKind] = &[
    END_KW, ELSE_KW, ELSIF_KW, IN_KW, WHEN_KW, R_BRACE, INTERP_END, EOF,
];

/// Keywords that start an item; in column 1 inside a nested body they close it (a
/// recovery hint for a missing `end`, ADR 0018 §6).
const ITEM_KEYWORDS: &[SyntaxKind] = &[DEF_KW, STRUCT_KW, ENUM_KW, TRAIT_KW, IMPL_KW, MODULE_KW];

pub(crate) fn source_file(p: &mut Parser<'_>) {
    let file = p.start();
    items::item_list(p, false);
    // Anything left (a stray closing token) was reported by `item_list`.
    while !p.at(EOF) {
        p.bump_error();
    }
    file.complete(p, SOURCE_FILE);
}

/// Whether the current token, in column 1, should close a nested body.
pub(crate) fn at_column_one_item(p: &Parser<'_>) -> bool {
    (p.at_any(ITEM_KEYWORDS) || p.at(SyntaxKind::PUB_KW)) && p.nth_at_line_start(0)
}

/// Expects the `end` that closes the block opened by the keyword at `opener`.
pub(crate) fn expect_end(p: &mut Parser<'_>, opener: TextRange) {
    if p.eat(END_KW) {
        return;
    }
    let keyword = p.text[opener.to_usize()].to_owned();
    let found = p.current().describe();
    let at = p.current_range();
    let diagnostic = loom_diagnostics::Diagnostic::error(
        codes::MISSING_END,
        p.span(opener),
        format!("missing `end` for this `{keyword}`"),
    )
    .with_label(format!("this `{keyword}` is never closed"))
    .with_secondary(p.span(at), format!("expected `end`, found {found}"));
    p.push_diagnostic(diagnostic);
}

/// After a statement or item: requires a line break (or a terminator), reporting
/// E0111 / E0103 / E0100 and skipping the rest of the line otherwise.
pub(crate) fn end_of_line(p: &mut Parser<'_>, terminators: &[SyntaxKind]) {
    if p.at(NEWLINE) || p.at(EOF) || p.at_any(terminators) {
        return;
    }
    // The first token on its line, without a `NEWLINE` before it: the lexer suppressed
    // the line break after an incomplete construct that was already reported (such as
    // `use a.*`). Treat it as a line break.
    if p.first_on_line(0) {
        return;
    }
    let range = p.current_range();
    if p.at_any(&[R_PAREN, R_BRACKET, R_BRACE, END_KW]) {
        p.error_at(
            codes::UNEXPECTED_CLOSING,
            range,
            format!("unexpected {}", p.current().describe()),
        );
    } else if expressions::at_statement_start(p) {
        let diagnostic = loom_diagnostics::Diagnostic::error(
            codes::STATEMENTS_ON_ONE_LINE,
            p.span(range),
            "two statements on one line",
        )
        .with_label("this starts a second statement")
        .with_help("put each statement on its own line");
        p.push_diagnostic(diagnostic);
        return;
    } else {
        p.error_expected("line break");
    }
    // Skip the rest of the line.
    let marker = p.start();
    while !p.at(NEWLINE) && !p.at(EOF) && !p.at_any(terminators) {
        p.bump();
    }
    marker.complete(p, SyntaxKind::ERROR);
}
