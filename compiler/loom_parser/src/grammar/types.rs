//! Types and paths (spec §5, §7.2).

use loom_syntax::SyntaxKind::{
    ARROW, COMMA, DOT, DYN_KW, DYN_TYPE, EOF, ERROR, FN_TYPE, GENERIC_ARG_LIST, IDENT, L_BRACKET,
    L_PAREN, MUT_KW, NAME_REF, PACKAGE_KW, PATH, PATH_SEGMENT, PATH_TYPE, R_BRACKET, R_PAREN,
    REF_KW, REF_TYPE, SELF_KW, SELF_TYPE_KW, SUPER_KW, TUPLE_TYPE,
};

use super::{expressions, items};
use crate::parser::Parser;

const PATH_STARTS: &[loom_syntax::SyntaxKind] =
    &[IDENT, SELF_KW, SELF_TYPE_KW, SUPER_KW, PACKAGE_KW];

pub(super) fn type_(p: &mut Parser<'_>) {
    if !p.enter() {
        return;
    }
    match p.current() {
        L_PAREN => tuple_type(p),
        DYN_KW => {
            let marker = p.start();
            p.bump();
            items::bounds(p);
            marker.complete(p, DYN_TYPE);
        }
        REF_KW | MUT_KW => {
            let marker = p.start();
            p.eat(MUT_KW);
            p.expect(REF_KW);
            type_(p);
            marker.complete(p, REF_TYPE);
        }
        IDENT if matches!(p.current_text(), "Fn" | "FnMut" | "FnOnce") && p.nth(1) == L_PAREN => {
            fn_type(p);
        }
        kind if PATH_STARTS.contains(&kind) => path_type(p),
        _ => {
            p.error_expected("a type");
            if !p.at_any(&[
                EOF,
                R_PAREN,
                R_BRACKET,
                COMMA,
                loom_syntax::SyntaxKind::NEWLINE,
            ]) {
                p.bump_error();
            }
        }
    }
    p.leave();
}

fn tuple_type(p: &mut Parser<'_>) {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_PAREN) && !p.at(EOF) {
        type_(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_PAREN, opener);
    marker.complete(p, TUPLE_TYPE);
}

fn fn_type(p: &mut Parser<'_>) {
    let marker = p.start();
    p.bump(); // Fn / FnMut / FnOnce
    let opener = p.current_range();
    p.bump(); // (
    p.push_newlines(false);
    while !p.at(R_PAREN) && !p.at(EOF) {
        type_(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_PAREN, opener);
    if p.eat(ARROW) {
        type_(p);
    }
    marker.complete(p, FN_TYPE);
}

/// `path [generic_args]`.
pub(super) fn path_type(p: &mut Parser<'_>) {
    let marker = p.start();
    path(p);
    if p.at(L_BRACKET) {
        generic_args(p);
    }
    marker.complete(p, PATH_TYPE);
}

/// `segment { "." segment }`.
pub(super) fn path(p: &mut Parser<'_>) {
    let marker = p.start();
    if !p.at_any(PATH_STARTS) {
        p.error_expected("a path");
        marker.complete(p, ERROR);
        return;
    }
    path_segment(p);
    while p.at(DOT) && PATH_STARTS.contains(&p.nth(1)) {
        p.bump();
        path_segment(p);
    }
    marker.complete(p, PATH);
}

pub(super) fn path_segment(p: &mut Parser<'_>) {
    let segment = p.start();
    let name = p.start();
    p.bump();
    name.complete(p, NAME_REF);
    segment.complete(p, PATH_SEGMENT);
}

fn generic_args(p: &mut Parser<'_>) {
    let marker = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_BRACKET) && !p.at(EOF) {
        type_(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_BRACKET, opener);
    marker.complete(p, GENERIC_ARG_LIST);
}
