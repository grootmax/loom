//! The canonical Loom formatter (no configuration options).
//!
//! The style is specified in `docs/reference/style.md`. The formatter works on the
//! lossless CST and only changes whitespace and line breaks: indentation is derived from
//! the tree, spacing from the kinds of neighboring tokens, and the author's line breaks
//! are kept (with blank lines normalized). Comments and string contents are never touched.

use std::collections::BTreeSet;

use loom_diagnostics::Diagnostic;
use loom_span::{FileId, LineIndex};
use loom_syntax::SyntaxKind::{
    self, ARG_LIST, ARRAY_EXPR, AT_BRACKET, ATTR_ARG_LIST, ATTRIBUTE, BANG, BLOCK,
    BLOCK_PARAM_LIST, COLON, COMMA, COMMENT, DO_KW, DOC_COMMENT, DOT, DOT2, DOT2_EQ, END_KW,
    ENUM_DEF, ERROR_TOKEN, EXTERN_BLOCK, FN_TYPE, GENERIC_ARG_LIST, GENERIC_PARAM_LIST, IMPL_DEF,
    INDEX_EXPR, INNER_DOC_COMMENT, INT_LIT, INTERP_END, INTERP_START, INTERPOLATION, L_BRACE,
    L_BRACKET, L_PAREN, LINE_BREAK, LITERAL_PAT, MAP_EXPR, MINUS, MODULE_DEF, NEWLINE, PARAM_LIST,
    PAREN_EXPR, PAREN_PAT, PIPE, PREFIX_EXPR, QUESTION, R_BRACE, R_BRACKET, R_PAREN, RANGE_EXPR,
    RANGE_PAT, SEMICOLON, SHEBANG, STRING_END, STRING_FRAGMENT, STRING_START, STRUCT_DEF, TILDE,
    TRAIT_DEF, TUPLE_EXPR, TUPLE_PAT, TUPLE_STRUCT_PAT, TUPLE_TYPE, USE_TREE_LIST,
    VARIANT_FIELD_LIST, VISIBILITY, WHITESPACE,
};
use loom_syntax::{SyntaxNode, SyntaxToken};

const INDENT: &str = "  ";

/// Parses and formats `source`. Returns the diagnostics instead if the file has errors.
///
/// # Errors
///
/// Returns every diagnostic if the source has lexical or syntax errors.
pub fn format_source(source: &str) -> Result<String, Vec<Diagnostic>> {
    let parse = loom_parser::parse(source, FileId::DETACHED);
    if parse.has_errors() {
        return Err(parse.diagnostics().to_vec());
    }
    Ok(format_tree(&parse.syntax()))
}

/// Formats a tree that has no syntax errors.
#[must_use]
pub fn format_tree(root: &SyntaxNode) -> String {
    let text = root.text();
    let lines_index = LineIndex::new(&text);
    let line_of = |token: &SyntaxToken| lines_index.line_of(token.text_range().start());

    // Split the token stream into source lines (at NEWLINE / LINE_BREAK tokens), keeping
    // the whitespace that preceded each token.
    let mut lines: Vec<Vec<(String, SyntaxToken)>> = vec![Vec::new()];
    let mut pending_space = String::new();
    for token in root.tokens() {
        match token.kind() {
            NEWLINE | LINE_BREAK => {
                lines.push(Vec::new());
                pending_space.clear();
            }
            WHITESPACE => pending_space.push_str(token.text()),
            _ => {
                let space = std::mem::take(&mut pending_space);
                if let Some(line) = lines.last_mut() {
                    line.push((space, token));
                }
            }
        }
    }

    // Indentation of each code line; comment-only lines follow the next code line.
    let mut indents: Vec<Option<usize>> = lines
        .iter()
        .map(|line| match line.first() {
            Some((_, token)) if !is_comment(token.kind()) => {
                Some(code_indent(token, &line_of, line))
            }
            _ => None,
        })
        .collect();
    let mut next_code: Option<(usize, bool)> = None;
    for index in (0..lines.len()).rev() {
        if let Some(indent) = indents[index] {
            let closes = lines[index].first().is_some_and(|(_, token)| {
                matches!(token.kind(), END_KW | R_BRACE | R_PAREN | R_BRACKET)
            });
            next_code = Some((indent, closes));
        } else if let Some((_, first)) = lines[index].first() {
            let indent = if first.kind() == SHEBANG {
                // Indenting a shebang would turn it into a comment.
                0
            } else {
                next_code.map_or(0, |(indent, closes)| indent + usize::from(closes))
            };
            indents[index] = Some(indent);
        }
    }

    // Emit, normalizing blank lines.
    let mut out = String::new();
    let mut previous_indent: Option<usize> = None;
    let mut blank_pending = false;
    for (index, line) in lines.iter().enumerate() {
        let Some(indent) = indents[index] else {
            blank_pending = previous_indent.is_some();
            continue;
        };
        let opens_or_closes = previous_indent.is_some_and(|previous| indent > previous)
            || line.first().is_some_and(|(_, token)| {
                matches!(token.kind(), END_KW | R_BRACE | R_PAREN | R_BRACKET)
            });
        if blank_pending && !opens_or_closes {
            out.push('\n');
        }
        blank_pending = false;
        for _ in 0..indent {
            out.push_str(INDENT);
        }
        emit_line(line, &mut out);
        out.push('\n');
        previous_indent = Some(indent);
    }
    out
}

fn is_comment(kind: SyntaxKind) -> bool {
    matches!(kind, COMMENT | DOC_COMMENT | INNER_DOC_COMMENT | SHEBANG)
}

fn emit_line(line: &[(String, SyntaxToken)], out: &mut String) {
    let mut previous: Option<&SyntaxToken> = None;
    for (space, token) in line {
        if let Some(previous) = previous {
            if is_comment(token.kind()) {
                // Keep the author's alignment before a trailing comment.
                out.push_str(if space.is_empty() { " " } else { space });
            } else if space_between(previous, token) {
                out.push(' ');
            }
        }
        if is_comment(token.kind()) {
            out.push_str(token.text().trim_end());
        } else {
            out.push_str(token.text());
        }
        previous = Some(token);
    }
}

// ----- indentation -----------------------------------------------------------------------

/// The token that opens an indenting container, or `None` if `node` doesn't indent.
fn container_opener(node: &SyntaxNode) -> Option<SyntaxToken> {
    match node.kind() {
        BLOCK => node.first_token(),
        STRUCT_DEF | ENUM_DEF | TRAIT_DEF | IMPL_DEF | MODULE_DEF | EXTERN_BLOCK => node
            .children_with_tokens()
            .filter_map(loom_syntax::SyntaxElement::into_token)
            .find(|token| token.kind().is_keyword() && token.kind() != SyntaxKind::PUB_KW),
        kind if is_bracket_container(kind) => node
            .children_with_tokens()
            .filter_map(loom_syntax::SyntaxElement::into_token)
            .find(|token| {
                matches!(
                    token.kind(),
                    L_PAREN | L_BRACKET | L_BRACE | AT_BRACKET | INTERP_START | PIPE
                )
            }),
        _ => None,
    }
}

fn is_bracket_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        ARG_LIST
            | PARAM_LIST
            | ARRAY_EXPR
            | MAP_EXPR
            | TUPLE_EXPR
            | PAREN_EXPR
            | GENERIC_PARAM_LIST
            | GENERIC_ARG_LIST
            | INDEX_EXPR
            | ATTR_ARG_LIST
            | ATTRIBUTE
            | VARIANT_FIELD_LIST
            | TUPLE_STRUCT_PAT
            | TUPLE_PAT
            | PAREN_PAT
            | TUPLE_TYPE
            | FN_TYPE
            | USE_TREE_LIST
            | BLOCK_PARAM_LIST
            | INTERPOLATION
    )
}

fn is_closer(kind: SyntaxKind) -> bool {
    matches!(kind, END_KW | R_PAREN | R_BRACKET | R_BRACE | INTERP_END)
}

fn code_indent(
    first: &SyntaxToken,
    line_of: &impl Fn(&SyntaxToken) -> usize,
    line: &[(String, SyntaxToken)],
) -> usize {
    let this_line = line_of(first);
    // Containers closed by the closing tokens this line starts with.
    let closed: Vec<SyntaxNode> = line
        .iter()
        .map(|(_, token)| token)
        .take_while(|token| is_closer(token.kind()))
        .map(SyntaxToken::parent)
        .collect();
    let mut levels = BTreeSet::new();
    let mut innermost_bracket: Option<SyntaxNode> = None;
    let mut seen_container = false;
    for ancestor in first.parent().ancestors() {
        let Some(opener) = container_opener(&ancestor) else {
            continue;
        };
        if !seen_container {
            seen_container = true;
            if is_bracket_container(ancestor.kind()) {
                innermost_bracket = Some(ancestor.clone());
            }
        }
        let opener_line = line_of(&opener);
        if opener_line < this_line && !closed.contains(&ancestor) {
            levels.insert(opener_line);
        }
    }
    levels.len() + usize::from(is_continuation(first, innermost_bracket.as_ref()))
}

/// Whether `first` continues a statement from an earlier line (spec §2.3 rule 1).
fn is_continuation(first: &SyntaxToken, innermost_bracket: Option<&SyntaxNode>) -> bool {
    if is_closer(first.kind()) {
        return false;
    }
    let mut cursor = first.prev_token();
    while let Some(token) = cursor {
        match token.kind() {
            WHITESPACE | LINE_BREAK | COMMENT | DOC_COMMENT | INNER_DOC_COMMENT | ERROR_TOKEN => {
                cursor = token.prev_token();
            }
            NEWLINE | L_PAREN | L_BRACKET | L_BRACE | AT_BRACKET | INTERP_START | DO_KW => {
                return false;
            }
            COMMA if innermost_bracket.is_some_and(|bracket| token.parent() == *bracket) => {
                return false;
            }
            PIPE if token.parent().kind() == BLOCK_PARAM_LIST => return false,
            _ => return true,
        }
    }
    false
}

// ----- spacing -----------------------------------------------------------------------------

/// Whether one space goes between two neighboring tokens on a line.
fn space_between(previous: &SyntaxToken, next: &SyntaxToken) -> bool {
    let (left, right) = (previous.kind(), next.kind());
    let next_parent = next.parent().kind();
    let previous_parent = previous.parent().kind();

    // String internals are printed verbatim.
    if matches!(right, STRING_FRAGMENT | STRING_END | INTERP_START)
        || matches!(left, STRING_START | STRING_FRAGMENT | INTERP_START)
        || right == INTERP_END
    {
        return false;
    }
    // `2 .0` is the integer 2 and field `0`; joining it into `2.0` would make a float
    // (spec §2.7.2 rule 3), so keep that space.
    if separates_number_from_field(previous, next) {
        return true;
    }
    if matches!(
        right,
        COMMA | R_PAREN | R_BRACKET | QUESTION | DOT | COLON | SEMICOLON
    ) {
        return false;
    }
    if matches!(left, L_PAREN | L_BRACKET | AT_BRACKET | DOT) {
        return false;
    }
    if right == L_PAREN
        && matches!(
            next_parent,
            ARG_LIST
                | PARAM_LIST
                | ATTR_ARG_LIST
                | VARIANT_FIELD_LIST
                | TUPLE_STRUCT_PAT
                | FN_TYPE
                | VISIBILITY
        )
    {
        return false;
    }
    if right == L_BRACKET
        && matches!(
            next_parent,
            GENERIC_PARAM_LIST | GENERIC_ARG_LIST | INDEX_EXPR
        )
    {
        return false;
    }
    if (left == L_BRACE && matches!(previous_parent, MAP_EXPR | USE_TREE_LIST))
        || (right == R_BRACE && matches!(next_parent, MAP_EXPR | USE_TREE_LIST))
    {
        return false;
    }
    if matches!(left, DOT2 | DOT2_EQ) && matches!(previous_parent, RANGE_EXPR | RANGE_PAT)
        || matches!(right, DOT2 | DOT2_EQ) && matches!(next_parent, RANGE_EXPR | RANGE_PAT)
    {
        return false;
    }
    if matches!(left, MINUS | TILDE | BANG) && matches!(previous_parent, PREFIX_EXPR | LITERAL_PAT)
    {
        return false;
    }
    if left == PIPE && previous_parent == BLOCK_PARAM_LIST && is_first_pipe(previous) {
        return false;
    }
    if right == PIPE && next_parent == BLOCK_PARAM_LIST && !is_first_pipe(next) {
        return false;
    }
    true
}

/// Whether joining `previous` and `next` would turn `INT . INT` into a float literal.
fn separates_number_from_field(previous: &SyntaxToken, next: &SyntaxToken) -> bool {
    let neighbor = |token: &SyntaxToken, forward: bool| {
        let step = |t: &SyntaxToken| {
            if forward {
                t.next_token()
            } else {
                t.prev_token()
            }
        };
        let mut cursor = step(token);
        while let Some(candidate) = cursor {
            if candidate.kind() != WHITESPACE {
                return Some(candidate);
            }
            cursor = step(&candidate);
        }
        None
    };
    let is_int = |token: Option<SyntaxToken>| token.is_some_and(|t| t.kind() == INT_LIT);
    (previous.kind() == INT_LIT && next.kind() == DOT && is_int(neighbor(next, true)))
        || (previous.kind() == DOT && next.kind() == INT_LIT && is_int(neighbor(previous, false)))
}

fn is_first_pipe(pipe: &SyntaxToken) -> bool {
    pipe.parent().token_of_kind(PIPE).as_ref() == Some(pipe)
}

#[cfg(test)]
mod tests;
