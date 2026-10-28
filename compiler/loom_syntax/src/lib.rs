//! Lossless concrete syntax tree (CST) shared by the compiler, formatter and LSP.
//!
//! The design follows rust-analyzer's `rowan` ([ADR 0008] §3, [ADR 0018] §5):
//!
//! - the **green tree** ([`GreenNode`], [`GreenToken`]) is immutable, knows only lengths
//!   (not positions), and is shared through `Arc`;
//! - the **red tree** ([`SyntaxNode`], [`SyntaxToken`]) is a set of cheap cursors over the
//!   green tree that add parent pointers and absolute offsets.
//!
//! Every byte of the source, including whitespace and comments, is in the tree, so the
//! text of the root always equals the source.
//!
//! [ADR 0008]: https://github.com/grootmax/loom/blob/main/docs/adr/0008-compiler-architecture.md
//! [ADR 0018]: https://github.com/grootmax/loom/blob/main/docs/adr/0018-front-end-design.md

mod green;
mod kind;
mod red;

pub use green::{Checkpoint, GreenElement, GreenNode, GreenNodeBuilder, GreenToken};
pub use kind::SyntaxKind;
pub use red::{SyntaxElement, SyntaxElementChildren, SyntaxNode, SyntaxToken};

#[cfg(test)]
mod tests {
    use super::{GreenNodeBuilder, SyntaxElement, SyntaxKind, SyntaxNode};
    use loom_span::TextSize;

    use SyntaxKind::{
        BINARY_EXPR, EQ, IDENT, INT_LIT, LITERAL, NAME, PATH_EXPR, PLUS, SOURCE_FILE, STAR,
        WHITESPACE,
    };

    /// Builds `x = 1 + 2 * 3` with a checkpoint for each binary expression.
    fn sample() -> SyntaxNode {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SOURCE_FILE);
        builder.start_node(NAME);
        builder.token(IDENT, "x");
        builder.finish_node();
        builder.token(WHITESPACE, " ");
        builder.token(EQ, "=");
        builder.token(WHITESPACE, " ");
        let outer = builder.checkpoint();
        builder.start_node(LITERAL);
        builder.token(INT_LIT, "1");
        builder.finish_node();
        builder.token(WHITESPACE, " ");
        builder.token(PLUS, "+");
        builder.token(WHITESPACE, " ");
        let inner = builder.checkpoint();
        builder.start_node(LITERAL);
        builder.token(INT_LIT, "2");
        builder.finish_node();
        builder.token(STAR, "*");
        builder.start_node(PATH_EXPR);
        builder.token(IDENT, "y");
        builder.finish_node();
        builder.start_node_at(inner, BINARY_EXPR);
        builder.finish_node();
        builder.start_node_at(outer, BINARY_EXPR);
        builder.finish_node();
        builder.finish_node();
        SyntaxNode::new_root(builder.finish())
    }

    #[test]
    fn tree_is_lossless() {
        assert_eq!(sample().text(), "x = 1 + 2*y");
    }

    #[test]
    fn dump_shows_structure_and_ranges() {
        let expected = r#"SOURCE_FILE@0..11
  NAME@0..1
    IDENT@0..1 "x"
  WHITESPACE@1..2 " "
  EQ@2..3 "="
  WHITESPACE@3..4 " "
  BINARY_EXPR@4..11
    LITERAL@4..5
      INT_LIT@4..5 "1"
    WHITESPACE@5..6 " "
    PLUS@6..7 "+"
    WHITESPACE@7..8 " "
    BINARY_EXPR@8..11
      LITERAL@8..9
        INT_LIT@8..9 "2"
      STAR@9..10 "*"
      PATH_EXPR@10..11
        IDENT@10..11 "y"
"#;
        assert_eq!(sample().debug_dump(), expected);
    }

    #[test]
    fn navigation() {
        let root = sample();
        let outer = root.child_of_kind(BINARY_EXPR).unwrap();
        let inner = outer.child_of_kind(BINARY_EXPR).unwrap();
        assert_eq!(inner.parent().unwrap(), outer);
        assert_eq!(inner.ancestors().count(), 3);
        assert_eq!(inner.text(), "2*y");
        assert_eq!(
            root.descendants().filter(|n| n.kind() == LITERAL).count(),
            2
        );
        let first = root.first_token().unwrap();
        assert_eq!(first.text(), "x");
        let tokens: Vec<String> = root.tokens().map(|t| t.text().to_owned()).collect();
        assert_eq!(tokens.concat(), root.text());

        // next_token / prev_token walk across node boundaries.
        let mut walked = Vec::new();
        let mut token = Some(first);
        while let Some(current) = token {
            walked.push(current.text().to_owned());
            token = current.next_token();
        }
        assert_eq!(walked, tokens);
        let last = root.last_token().unwrap();
        assert_eq!(last.text(), "y");
        assert_eq!(last.prev_token().unwrap().kind(), STAR);

        let plus = outer.token_of_kind(PLUS).unwrap();
        assert_eq!(plus.text_range().start().raw(), 6);
        assert!(matches!(
            plus.next_sibling_or_token(),
            Some(SyntaxElement::Token(ref t)) if t.kind() == WHITESPACE
        ));
    }

    #[test]
    fn token_at_offset() {
        let root = sample();
        let at = |offset| {
            root.token_at_offset(TextSize::new(offset))
                .map(|t| t.kind())
        };
        assert_eq!(at(0), Some(IDENT));
        assert_eq!(at(2), Some(EQ));
        assert_eq!(at(9), Some(STAR));
        assert_eq!(at(11), Some(IDENT)); // end of file belongs to the last token
        assert_eq!(at(12), None);
    }

    #[test]
    fn red_nodes_compare_by_identity_and_position() {
        let root = sample();
        let a = root.child_of_kind(BINARY_EXPR).unwrap();
        let b = root.child_of_kind(BINARY_EXPR).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, a.child_of_kind(BINARY_EXPR).unwrap());
    }

    #[test]
    #[should_panic(expected = "checkpoint is outside the current node")]
    fn checkpoint_outside_current_node_panics() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SOURCE_FILE);
        builder.token(IDENT, "a");
        let checkpoint = builder.checkpoint();
        builder.token(IDENT, "b");
        builder.start_node(NAME);
        builder.start_node_at(checkpoint, LITERAL);
    }
}
