//! Items (spec §3).

use loom_diagnostics::{Diagnostic, codes};
use loom_syntax::SyntaxKind::{
    self, ARROW, AS_KW, ASSOC_TYPE, AT_BRACKET, ATTR_ARG, ATTR_ARG_LIST, ATTR_ENTRY, ATTRIBUTE,
    BOUND_LIST, COLON, COMMA, CONST_DEF, CONST_KW, DEF_KW, DOT, END_KW, ENUM_DEF, ENUM_KW, EOF, EQ,
    ERROR, EXTERN_BLOCK, EXTERN_KW, FIELD_DEF, FN_DEF, FOR_KW, GENERIC_PARAM, GENERIC_PARAM_LIST,
    IDENT, IMPL_DEF, IMPL_KW, L_BRACE, L_BRACKET, L_PAREN, MODULE_DEF, MODULE_KW, MUT_KW, NAME,
    NEWLINE, OWN_KW, PACKAGE_KW, PARAM, PARAM_LIST, PLUS, PUB_KW, R_BRACE, R_BRACKET, R_PAREN,
    RET_TYPE, SELF_KW, SELF_TYPE_KW, STAR, STRING_START, STRUCT_DEF, STRUCT_KW, SUPER_KW,
    TRAIT_DEF, TRAIT_KW, TYPE_ALIAS, TYPE_KW, USE_DECL, USE_KW, USE_TREE, USE_TREE_LIST,
    VARIANT_DEF, VARIANT_FIELD_LIST, VISIBILITY, WHERE_CLAUSE, WHERE_KW, WHERE_PRED,
};

use super::{at_column_one_item, end_of_line, expect_end, expressions, types};
use crate::parser::Parser;

/// Where a list of members is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Container {
    SourceFile,
    Module,
    Struct,
    Enum,
    Trait,
    Impl,
    Extern,
}

/// `{ item nl }` until `end` (inside a module) or the end of the file.
pub(super) fn item_list(p: &mut Parser<'_>, in_module: bool) {
    let container = if in_module {
        Container::Module
    } else {
        Container::SourceFile
    };
    member_list(p, container);
}

fn member_list(p: &mut Parser<'_>, container: Container) {
    let nested = container != Container::SourceFile;
    loop {
        p.eat_newlines();
        if p.at(EOF) || (nested && p.at(END_KW)) {
            break;
        }
        if nested && at_column_one_item(p) {
            // An item keyword in column 1 inside a body: the body is probably missing
            // its `end` (a recovery hint only, ADR 0018 §6).
            break;
        }
        if p.at(END_KW) {
            let range = p.current_range();
            p.error_at(codes::UNEXPECTED_CLOSING, range, "unexpected `end`");
            p.bump_error();
            continue;
        }
        if p.at_any(&[R_PAREN, R_BRACKET, R_BRACE]) {
            let range = p.current_range();
            p.error_at(
                codes::UNEXPECTED_CLOSING,
                range,
                format!("unexpected {}", p.current().describe()),
            );
            p.bump_error();
            continue;
        }
        member(p, container);
        end_of_line(p, &[END_KW]);
    }
}

fn member(p: &mut Parser<'_>, container: Container) {
    let marker = p.start();
    attributes(p);
    visibility(p);
    let kind = match (container, p.current()) {
        (_, DEF_KW) => {
            let signature_only = matches!(container, Container::Trait | Container::Extern);
            fn_def(p, signature_only)
        }
        (Container::Trait, IDENT) if p.at_contextual("default") && p.nth(1) == DEF_KW => {
            p.bump();
            fn_def(p, false)
        }
        (Container::Struct, IDENT) => field_def(p),
        (Container::Enum, IDENT) => variant_def(p),
        (Container::Trait, TYPE_KW) => assoc_type(p),
        (Container::Impl | Container::SourceFile | Container::Module, TYPE_KW) => type_alias(p),
        (_, CONST_KW) => const_def(p),
        (Container::SourceFile | Container::Module, kind) => match kind {
            STRUCT_KW => struct_def(p),
            ENUM_KW => enum_def(p),
            TRAIT_KW => trait_def(p),
            IMPL_KW => impl_def(p),
            MODULE_KW => module_def(p),
            USE_KW => use_decl(p),
            EXTERN_KW => extern_block(p),
            _ => {
                p.error_expected("an item (`def`, `struct`, `enum`, `trait`, `impl`, …)");
                skip_line(p);
                ERROR
            }
        },
        _ => {
            let what = match container {
                Container::Struct => "a field or `def`",
                Container::Enum => "a variant or `def`",
                Container::Trait => "`def`, `default def`, `type` or `const`",
                Container::Impl => "`def`, `type` or `const`",
                _ => "`def`",
            };
            p.error_expected(what);
            skip_line(p);
            ERROR
        }
    };
    marker.complete(p, kind);
}

fn skip_line(p: &mut Parser<'_>) {
    while !p.at(NEWLINE) && !p.at(EOF) && !p.at(END_KW) {
        p.bump();
    }
}

// ----- attributes and visibility (§3.10, §3.11) --------------------------------------------

fn attributes(p: &mut Parser<'_>) {
    while p.at(AT_BRACKET) {
        let attribute = p.start();
        let opener = p.current_range();
        p.bump();
        p.push_newlines(false);
        loop {
            attr_entry(p);
            if !p.eat(COMMA) || p.at(R_BRACKET) {
                break;
            }
        }
        p.pop_newlines();
        expressions::expect_closing(p, R_BRACKET, opener);
        attribute.complete(p, ATTRIBUTE);
        p.eat_newlines();
    }
}

fn attr_entry(p: &mut Parser<'_>) {
    let entry = p.start();
    p.expect(IDENT);
    if p.at(L_PAREN) {
        let list = p.start();
        let opener = p.current_range();
        p.bump();
        while !p.at(R_PAREN) && !p.at(EOF) {
            let arg = p.start();
            if p.at(IDENT) && p.nth(1) == COLON {
                p.bump();
                p.bump();
            }
            expressions::expr(p);
            arg.complete(p, ATTR_ARG);
            if !p.eat(COMMA) {
                break;
            }
        }
        expressions::expect_closing(p, R_PAREN, opener);
        list.complete(p, ATTR_ARG_LIST);
    }
    entry.complete(p, ATTR_ENTRY);
}

fn visibility(p: &mut Parser<'_>) {
    if !p.at(PUB_KW) {
        return;
    }
    let marker = p.start();
    p.bump();
    if p.at(L_PAREN) && p.nth(1) == PACKAGE_KW {
        p.bump();
        p.bump();
        p.expect(R_PAREN);
    }
    marker.complete(p, VISIBILITY);
}

/// `NAME`: a single identifier being declared.
pub(super) fn name(p: &mut Parser<'_>) -> bool {
    if p.at(IDENT) {
        let marker = p.start();
        p.bump();
        marker.complete(p, NAME);
        true
    } else {
        p.error_expected("a name");
        false
    }
}

// ----- functions (§3.1) ------------------------------------------------------------------

fn fn_def(p: &mut Parser<'_>, signature_only: bool) -> SyntaxKind {
    let opener = p.current_range();
    p.bump(); // def
    if p.at(MUT_KW) || p.at(OWN_KW) {
        p.bump();
    }
    if p.at(SELF_KW) && p.nth(1) == DOT {
        p.bump();
        p.bump();
    }
    if !name(p) && !p.at_any(&[L_PAREN, L_BRACKET, ARROW, EQ]) {
        return FN_DEF;
    }
    if p.at(L_BRACKET) {
        generic_params(p);
    }
    if p.at(L_PAREN) {
        param_list(p);
    }
    if p.at(ARROW) {
        let ret = p.start();
        p.bump();
        types::type_(p);
        ret.complete(p, RET_TYPE);
    }
    if p.at(WHERE_KW) {
        where_clause(p);
    }
    if p.eat(EQ) {
        expressions::expr(p);
    } else if signature_only {
        // A required method or extern function: no body.
    } else if p.at(NEWLINE) || p.at(END_KW) || p.at(EOF) {
        expressions::block_body(p);
        expect_end(p, opener);
    } else {
        p.error_expected("`=`, `->` or a line break");
    }
    FN_DEF
}

fn param_list(p: &mut Parser<'_>) {
    let list = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while !p.at(R_PAREN) && !p.at(EOF) {
        if !p.at_any(&[IDENT, MUT_KW, OWN_KW]) {
            p.error_expected("a parameter");
            break;
        }
        let param = p.start();
        if p.at(MUT_KW) || p.at(OWN_KW) {
            p.bump();
        }
        name(p);
        if p.expect(COLON) {
            types::type_(p);
        }
        if p.eat(EQ) {
            expressions::expr(p);
        }
        param.complete(p, PARAM);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_PAREN, opener);
    list.complete(p, PARAM_LIST);
}

pub(super) fn generic_params(p: &mut Parser<'_>) {
    let list = p.start();
    let opener = p.current_range();
    p.bump();
    p.push_newlines(false);
    while p.at(IDENT) {
        let param = p.start();
        name(p);
        if p.eat(COLON) {
            bounds(p);
        }
        param.complete(p, GENERIC_PARAM);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.pop_newlines();
    expressions::expect_closing(p, R_BRACKET, opener);
    list.complete(p, GENERIC_PARAM_LIST);
}

pub(super) fn bounds(p: &mut Parser<'_>) {
    let list = p.start();
    types::path_type(p);
    while p.eat(PLUS) {
        types::path_type(p);
    }
    list.complete(p, BOUND_LIST);
}

fn where_clause(p: &mut Parser<'_>) {
    let clause = p.start();
    p.bump();
    loop {
        let predicate = p.start();
        types::type_(p);
        if p.expect(COLON) {
            bounds(p);
        }
        predicate.complete(p, WHERE_PRED);
        if !p.eat(COMMA) {
            break;
        }
    }
    clause.complete(p, WHERE_CLAUSE);
}

// ----- types: struct, enum, trait, impl (§3.2–§3.5) ---------------------------------------

/// The part shared by `struct`, `enum` and `trait` headers: name and generics.
fn type_header(p: &mut Parser<'_>) {
    name(p);
    if p.at(L_BRACKET) {
        generic_params(p);
    }
}

fn body(p: &mut Parser<'_>, container: Container, opener: loom_span::TextRange) {
    if p.at(WHERE_KW) {
        where_clause(p);
    }
    if !p.at(NEWLINE) && !p.at(END_KW) && !p.at(EOF) {
        p.error_expected("a line break");
        skip_line(p);
    }
    member_list(p, container);
    expect_end(p, opener);
}

fn struct_def(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    type_header(p);
    body(p, Container::Struct, opener);
    STRUCT_DEF
}

fn field_def(p: &mut Parser<'_>) -> SyntaxKind {
    name(p);
    if p.expect(COLON) {
        types::type_(p);
    }
    FIELD_DEF
}

fn enum_def(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    type_header(p);
    body(p, Container::Enum, opener);
    ENUM_DEF
}

fn variant_def(p: &mut Parser<'_>) -> SyntaxKind {
    name(p);
    if p.at(L_PAREN) {
        let list = p.start();
        let opener = p.current_range();
        p.bump();
        p.push_newlines(false);
        while !p.at(R_PAREN) && !p.at(EOF) {
            if p.at(IDENT) && p.nth(1) == COLON {
                let field = p.start();
                field_def(p);
                field.complete(p, FIELD_DEF);
            } else {
                types::type_(p);
            }
            if !p.eat(COMMA) {
                break;
            }
        }
        p.pop_newlines();
        expressions::expect_closing(p, R_PAREN, opener);
        list.complete(p, VARIANT_FIELD_LIST);
    }
    VARIANT_DEF
}

fn trait_def(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    type_header(p);
    if p.eat(COLON) {
        bounds(p);
    }
    body(p, Container::Trait, opener);
    TRAIT_DEF
}

fn assoc_type(p: &mut Parser<'_>) -> SyntaxKind {
    p.bump();
    name(p);
    if p.eat(COLON) {
        bounds(p);
    }
    ASSOC_TYPE
}

fn impl_def(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    if p.at(L_BRACKET) {
        generic_params(p);
    }
    types::path_type(p);
    if p.eat(FOR_KW) {
        types::type_(p);
    } else {
        let diagnostic = Diagnostic::error(
            codes::INHERENT_IMPL,
            p.span(opener),
            "`impl` without a trait",
        )
        .with_label("expected `impl Trait for Type`")
        .with_help("inherent methods are written inside the type's own `struct` or `enum` body");
        p.push_diagnostic(diagnostic);
    }
    body(p, Container::Impl, opener);
    IMPL_DEF
}

// ----- modules, use, const, type, extern (§3.6–§3.9, §13) ----------------------------------

fn module_def(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    name(p);
    if !p.at(NEWLINE) && !p.at(END_KW) && !p.at(EOF) {
        p.error_expected("a line break");
        skip_line(p);
    }
    item_list(p, true);
    expect_end(p, opener);
    MODULE_DEF
}

fn use_decl(p: &mut Parser<'_>) -> SyntaxKind {
    p.bump();
    use_tree(p);
    USE_DECL
}

fn use_tree(p: &mut Parser<'_>) {
    let tree = p.start();
    let path = p.start();
    let mut segments = 0;
    loop {
        if p.at_any(&[IDENT, SELF_KW, SELF_TYPE_KW, SUPER_KW, PACKAGE_KW]) {
            types::path_segment(p);
            segments += 1;
        } else {
            p.error_expected("a path segment");
            break;
        }
        if p.at(DOT) && p.nth(1) != L_BRACE && p.nth(1) != STAR {
            p.bump();
        } else {
            break;
        }
    }
    if segments > 0 {
        path.complete(p, SyntaxKind::PATH);
    } else {
        path.abandon(p);
    }
    if p.at(DOT) && p.nth(1) == L_BRACE {
        p.bump();
        let list = p.start();
        let opener = p.current_range();
        p.bump();
        p.push_newlines(false);
        while !p.at(R_BRACE) && !p.at(EOF) {
            use_tree(p);
            if !p.eat(COMMA) {
                break;
            }
        }
        p.pop_newlines();
        expressions::expect_closing(p, R_BRACE, opener);
        list.complete(p, USE_TREE_LIST);
    } else if p.at(DOT) && p.nth(1) == STAR {
        p.bump();
        let range = p.current_range();
        let diagnostic = Diagnostic::error(
            codes::GLOB_IMPORT,
            p.span(range),
            "glob imports are not supported",
        )
        .with_label("not supported")
        .with_help("name each item: `use path.{a, b}`");
        p.push_diagnostic(diagnostic);
        p.bump();
    } else if p.eat(AS_KW) {
        name(p);
    }
    tree.complete(p, USE_TREE);
}

fn const_def(p: &mut Parser<'_>) -> SyntaxKind {
    p.bump();
    name(p);
    if p.expect(COLON) {
        types::type_(p);
    }
    if p.expect(EQ) {
        expressions::expr(p);
    }
    CONST_DEF
}

fn type_alias(p: &mut Parser<'_>) -> SyntaxKind {
    p.bump();
    name(p);
    if p.at(L_BRACKET) {
        generic_params(p);
    }
    if p.expect(EQ) {
        types::type_(p);
    }
    TYPE_ALIAS
}

fn extern_block(p: &mut Parser<'_>) -> SyntaxKind {
    let opener = p.current_range();
    p.bump();
    if p.at(STRING_START) {
        expressions::expr(p);
    } else {
        p.error_expected("an ABI string such as \"C\"");
    }
    body(p, Container::Extern, opener);
    EXTERN_BLOCK
}
