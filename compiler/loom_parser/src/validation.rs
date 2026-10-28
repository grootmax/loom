//! Checks on the finished tree that are easier there than during parsing: doc comment
//! placement (spec §2.4).

use loom_diagnostics::{Diagnostic, codes};
use loom_span::{FileId, Span};
use loom_syntax::SyntaxKind::{
    ASSOC_TYPE, CONST_DEF, DOC_COMMENT, ENUM_DEF, EXTERN_BLOCK, FIELD_DEF, FN_DEF, IMPL_DEF,
    INNER_DOC_COMMENT, MODULE_DEF, SOURCE_FILE, STRUCT_DEF, TRAIT_DEF, TYPE_ALIAS, USE_DECL,
    VARIANT_DEF,
};
use loom_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

fn is_documentable(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        FN_DEF
            | STRUCT_DEF
            | ENUM_DEF
            | TRAIT_DEF
            | IMPL_DEF
            | MODULE_DEF
            | USE_DECL
            | CONST_DEF
            | TYPE_ALIAS
            | EXTERN_BLOCK
            | FIELD_DEF
            | VARIANT_DEF
            | ASSOC_TYPE
    )
}

pub(crate) fn validate(root: &SyntaxNode, file: FileId) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for token in root.tokens() {
        match token.kind() {
            DOC_COMMENT if !documents_an_item(&token) => {
                diagnostics.push(
                    Diagnostic::warning(
                        codes::DANGLING_DOC_COMMENT,
                        Span::new(file, token.text_range()),
                        "doc comment is not followed by an item, field, variant or method",
                    )
                    .with_help("use `#` for an ordinary comment"),
                );
            }
            INNER_DOC_COMMENT if follows_an_item(&token) => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::MISPLACED_INNER_DOC,
                        Span::new(file, token.text_range()),
                        "inner doc comment (`##!`) after an item",
                    )
                    .with_label("must come before the first item")
                    .with_help(
                        "move it to the top of the module, or use `##` to document the next item",
                    ),
                );
            }
            _ => {}
        }
    }
    diagnostics
}

/// Whether a doc comment was attached to a documentable node, before its first
/// significant token.
fn documents_an_item(token: &SyntaxToken) -> bool {
    let parent = token.parent();
    if !is_documentable(parent.kind()) {
        return false;
    }
    let first_significant = parent
        .tokens()
        .find(|t| !t.kind().is_trivia() && t.kind() != SyntaxKind::ERROR_TOKEN);
    first_significant.is_some_and(|first| token.text_range().start() < first.text_range().start())
}

/// Whether an item of the enclosing module (file or inline `module`) starts before the
/// token.
fn follows_an_item(token: &SyntaxToken) -> bool {
    let Some(module) = token
        .parent()
        .ancestors()
        .find(|node| matches!(node.kind(), SOURCE_FILE | MODULE_DEF))
    else {
        return false;
    };
    let offset = token.text_range().start();
    module
        .children()
        .filter(|child| is_documentable(child.kind()))
        .filter(|child| !child.text_range().contains(offset))
        .any(|child| child.text_range().start() < offset)
}
