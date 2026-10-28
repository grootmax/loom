//! Patterns (spec §9).

use loom_syntax::SyntaxKind::{
    self, CHAR_LIT, COLON, COMMA, DOT, DOT2, DOT2_EQ, EOF, FALSE_KW, FIELD_PAT, FLOAT_LIT, IDENT,
    IDENT_PAT, INT_LIT, L_PAREN, LITERAL_PAT, MINUS, NAME, OR_PAT, PAREN_PAT, PATH_PAT, PIPE,
    R_PAREN, RANGE_PAT, RAW_STRING_LIT, REST_PAT, SELF_KW, SELF_TYPE_KW, STRING_START, SUPER_KW,
    TRUE_KW, TUPLE_PAT, TUPLE_STRUCT_PAT, WILDCARD_PAT,
};

use super::{expressions, types};
use crate::parser::{CompletedMarker, Parser};

/// `pattern_atom { "|" pattern_atom }`.
pub(super) fn pattern(p: &mut Parser<'_>) {
    let Some(first) = pattern_atom(p) else {
        return;
    };
    if p.at(PIPE) {
        let marker = first.precede(p);
        while p.eat(PIPE) {
            pattern_atom(p);
        }
        marker.complete(p, OR_PAT);
    }
}

pub(super) fn pattern_atom(p: &mut Parser<'_>) -> Option<CompletedMarker> {
    if !p.enter() {
        return None;
    }
    let result = match p.current() {
        IDENT if p.current_text() == "_" => {
            let marker = p.start();
            p.bump();
            Some(marker.complete(p, WILDCARD_PAT))
        }
        IDENT if p.nth(1) != DOT && p.nth(1) != L_PAREN => {
            let marker = p.start();
            let name = p.start();
            p.bump();
            name.complete(p, NAME);
            Some(marker.complete(p, IDENT_PAT))
        }
        IDENT | SELF_KW | SELF_TYPE_KW | SUPER_KW => Some(path_pattern(p)),
        L_PAREN => Some(tuple_pattern(p)),
        INT_LIT | FLOAT_LIT | CHAR_LIT | STRING_START | RAW_STRING_LIT | TRUE_KW | FALSE_KW
        | MINUS => Some(literal_or_range(p)),
        _ => {
            p.error_expected("a pattern");
            None
        }
    };
    p.leave();
    result
}

fn literal_pattern(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    if p.eat(MINUS) && !p.at(INT_LIT) && !p.at(FLOAT_LIT) {
        p.error_expected("a number");
    }
    if p.at(STRING_START) {
        expressions::string(p);
    } else if p.at_any(&[
        INT_LIT,
        FLOAT_LIT,
        CHAR_LIT,
        RAW_STRING_LIT,
        TRUE_KW,
        FALSE_KW,
    ]) {
        p.bump();
    }
    marker.complete(p, LITERAL_PAT)
}

fn literal_or_range(p: &mut Parser<'_>) -> CompletedMarker {
    let start = literal_pattern(p);
    if p.at(DOT2) || p.at(DOT2_EQ) {
        let marker = start.precede(p);
        p.bump();
        literal_pattern(p);
        return marker.complete(p, RANGE_PAT);
    }
    start
}

fn path_pattern(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    types::path(p);
    if !p.at(L_PAREN) {
        return marker.complete(p, PATH_PAT);
    }
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_PAREN) && !p.at(EOF) {
        field_pattern(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_PAREN, opener);
    marker.complete(p, TUPLE_STRUCT_PAT)
}

fn field_pattern(p: &mut Parser<'_>) {
    if p.at(DOT2) {
        let marker = p.start();
        p.bump();
        marker.complete(p, REST_PAT);
    } else if p.at(IDENT) && p.nth(1) == COLON {
        let marker = p.start();
        let name = p.start();
        p.bump();
        name.complete(p, NAME);
        p.bump(); // :
        if !p.at(COMMA) && !p.at(R_PAREN) {
            pattern(p);
        }
        marker.complete(p, FIELD_PAT);
    } else {
        pattern(p);
    }
}

fn tuple_pattern(p: &mut Parser<'_>) -> CompletedMarker {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    let mut count = 0;
    let mut trailing_comma = false;
    while !p.at(R_PAREN) && !p.at(EOF) {
        if p.at(DOT2) {
            let rest = p.start();
            p.bump();
            rest.complete(p, REST_PAT);
        } else {
            pattern(p);
        }
        count += 1;
        trailing_comma = p.eat(COMMA);
        if !trailing_comma {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_PAREN, opener);
    let kind: SyntaxKind = if count == 1 && !trailing_comma {
        PAREN_PAT
    } else {
        TUPLE_PAT
    };
    marker.complete(p, kind)
}
