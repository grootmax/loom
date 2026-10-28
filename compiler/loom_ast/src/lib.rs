//! Typed AST views over the CST.
//!
//! Every AST type wraps a [`SyntaxNode`] of one kind and adds accessors, as in
//! rust-analyzer ([ADR 0008] §4). There is no separate tree: an AST node *is* a CST node,
//! so it keeps positions, trivia and errors, and costs nothing to create.
//!
//! Accessors return `Option` because the tree may be incomplete (the parser recovers
//! from errors).
//!
//! [ADR 0008]: https://github.com/grootmax/loom/blob/main/docs/adr/0008-compiler-architecture.md

mod expressions;
mod items;
mod literals;
mod patterns;
mod types;

pub use expressions::*;
pub use items::*;
pub use literals::*;
pub use patterns::*;
pub use types::*;

use loom_syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// A typed view of a CST node.
pub trait AstNode: Sized {
    /// Whether a node of `kind` can be viewed as `Self`.
    fn can_cast(kind: SyntaxKind) -> bool;

    /// Views `node` as `Self` if its kind matches.
    fn cast(node: SyntaxNode) -> Option<Self>;

    /// The underlying CST node.
    fn syntax(&self) -> &SyntaxNode;
}

/// The first child of `parent` that can be viewed as `N`.
fn child<N: AstNode>(parent: &SyntaxNode) -> Option<N> {
    parent.children().find_map(N::cast)
}

/// Every child of `parent` that can be viewed as `N`.
fn children<N: AstNode>(parent: &SyntaxNode) -> impl Iterator<Item = N> + use<N> {
    parent.children().filter_map(N::cast)
}

/// The first child token of `parent` with the given kind.
fn token(parent: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    parent.token_of_kind(kind)
}

/// The first significant child token of `parent` (not trivia, not a line break).
fn first_significant_token(parent: &SyntaxNode) -> Option<SyntaxToken> {
    parent
        .children_with_tokens()
        .filter_map(loom_syntax::SyntaxElement::into_token)
        .find(|token| {
            !token.kind().is_trivia()
                && token.kind() != SyntaxKind::NEWLINE
                && token.kind() != SyntaxKind::ERROR_TOKEN
        })
}

/// Declares a struct view for one node kind.
macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident, $kind:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(loom_syntax::SyntaxNode);

        impl $crate::AstNode for $name {
            fn can_cast(kind: loom_syntax::SyntaxKind) -> bool {
                kind == loom_syntax::SyntaxKind::$kind
            }

            fn cast(node: loom_syntax::SyntaxNode) -> Option<Self> {
                Self::can_cast(node.kind()).then(|| Self(node))
            }

            fn syntax(&self) -> &loom_syntax::SyntaxNode {
                &self.0
            }
        }
    };
}

/// Declares an enum view over several node kinds.
macro_rules! ast_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $(
                #[doc = concat!("A [`", stringify!($variant), "`].")]
                $variant($variant),
            )*
        }

        impl $crate::AstNode for $name {
            fn can_cast(kind: loom_syntax::SyntaxKind) -> bool {
                $(<$variant as $crate::AstNode>::can_cast(kind))||*
            }

            fn cast(node: loom_syntax::SyntaxNode) -> Option<Self> {
                $(
                    if <$variant as $crate::AstNode>::can_cast(node.kind()) {
                        return <$variant as $crate::AstNode>::cast(node).map(Self::$variant);
                    }
                )*
                None
            }

            fn syntax(&self) -> &loom_syntax::SyntaxNode {
                match self {
                    $(Self::$variant(node) => $crate::AstNode::syntax(node),)*
                }
            }
        }
    };
}

pub(crate) use {ast_enum, ast_node};

ast_node!(
    /// A whole file: `source_file`.
    SourceFile, SOURCE_FILE
);

impl SourceFile {
    /// The file's items.
    pub fn items(&self) -> impl Iterator<Item = Item> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// A declared name.
    Name, NAME
);

impl Name {
    /// The identifier token.
    #[must_use]
    pub fn ident(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::IDENT)
    }

    /// The name's text.
    #[must_use]
    pub fn text(&self) -> String {
        self.ident()
            .map(|t| t.text().to_owned())
            .unwrap_or_default()
    }
}

ast_node!(
    /// A reference to a name: an identifier, `self`, `Self`, `super` or `package`.
    NameRef, NAME_REF
);

impl NameRef {
    /// The referring token.
    #[must_use]
    pub fn token(&self) -> Option<SyntaxToken> {
        first_significant_token(&self.0)
    }

    /// The referenced text.
    #[must_use]
    pub fn text(&self) -> String {
        self.token()
            .map(|t| t.text().to_owned())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests;
