//! The green tree: immutable, position-independent, shareable CST nodes.

use std::fmt;
use std::sync::Arc;

use loom_span::TextSize;

use crate::SyntaxKind;

/// A token in the green tree: a kind and its exact text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GreenToken(Arc<GreenTokenData>);

#[derive(PartialEq, Eq, Hash)]
struct GreenTokenData {
    kind: SyntaxKind,
    text: Box<str>,
}

impl GreenToken {
    /// Creates a token.
    #[must_use]
    pub fn new(kind: SyntaxKind, text: &str) -> Self {
        Self(Arc::new(GreenTokenData {
            kind,
            text: text.into(),
        }))
    }

    /// The token's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    /// The token's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.0.text
    }

    /// The length of the token's text.
    #[must_use]
    pub fn text_len(&self) -> TextSize {
        TextSize::of(&self.0.text)
    }
}

impl fmt::Debug for GreenToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {:?}", self.kind(), self.text())
    }
}

/// A child of a green node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GreenElement {
    /// A nested node.
    Node(GreenNode),
    /// A token.
    Token(GreenToken),
}

impl GreenElement {
    /// The element's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind(),
            Self::Token(token) => token.kind(),
        }
    }

    /// The length of the element's text.
    #[must_use]
    pub fn text_len(&self) -> TextSize {
        match self {
            Self::Node(node) => node.text_len(),
            Self::Token(token) => token.text_len(),
        }
    }
}

/// A node in the green tree: a kind, its children and the cached length of its text.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct GreenNode(Arc<GreenNodeData>);

#[derive(PartialEq, Eq, Hash)]
struct GreenNodeData {
    kind: SyntaxKind,
    text_len: TextSize,
    children: Box<[GreenElement]>,
}

impl GreenNode {
    /// Creates a node from its children.
    #[must_use]
    pub fn new(kind: SyntaxKind, children: Vec<GreenElement>) -> Self {
        let text_len = children
            .iter()
            .fold(TextSize::new(0), |total, child| total + child.text_len());
        Self(Arc::new(GreenNodeData {
            kind,
            text_len,
            children: children.into_boxed_slice(),
        }))
    }

    /// The node's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.kind
    }

    /// The length of the node's text.
    #[must_use]
    pub fn text_len(&self) -> TextSize {
        self.0.text_len
    }

    /// The node's children.
    #[must_use]
    pub fn children(&self) -> &[GreenElement] {
        &self.0.children
    }

    /// Whether two handles point at the same node in memory.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Appends the node's full text to `out`.
    pub fn write_text(&self, out: &mut String) {
        for child in self.children() {
            match child {
                GreenElement::Node(node) => node.write_text(out),
                GreenElement::Token(token) => out.push_str(token.text()),
            }
        }
    }
}

impl fmt::Debug for GreenNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GreenNode")
            .field("kind", &self.kind())
            .field("text_len", &self.text_len())
            .field("children", &self.children())
            .finish()
    }
}

/// A position in a [`GreenNodeBuilder`] that a node can later be started at, to wrap
/// what was built since (used for left-recursive constructs such as binary expressions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint(usize);

/// Builds a green tree bottom-up.
#[derive(Debug, Default)]
pub struct GreenNodeBuilder {
    parents: Vec<(SyntaxKind, usize)>,
    children: Vec<GreenElement>,
}

impl GreenNodeBuilder {
    /// Creates an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a node. Every following token and node becomes its child until the matching
    /// [`finish_node`](Self::finish_node).
    pub fn start_node(&mut self, kind: SyntaxKind) {
        self.parents.push((kind, self.children.len()));
    }

    /// Adds a token to the current node.
    pub fn token(&mut self, kind: SyntaxKind, text: &str) {
        self.children
            .push(GreenElement::Token(GreenToken::new(kind, text)));
    }

    /// Finishes the current node.
    ///
    /// # Panics
    ///
    /// Panics if there is no unfinished node.
    pub fn finish_node(&mut self) {
        let (kind, first_child) = self.parents.pop().expect("finish_node without start_node");
        let children = self.children.split_off(first_child);
        self.children
            .push(GreenElement::Node(GreenNode::new(kind, children)));
    }

    /// Marks the current position.
    #[must_use]
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint(self.children.len())
    }

    /// Starts a node at `checkpoint`, so that everything added to the current node since
    /// then becomes the new node's children.
    ///
    /// # Panics
    ///
    /// Panics if `checkpoint` lies before the start of the current node, or after the
    /// current position.
    pub fn start_node_at(&mut self, checkpoint: Checkpoint, kind: SyntaxKind) {
        let Checkpoint(position) = checkpoint;
        assert!(
            position <= self.children.len(),
            "checkpoint is in the future"
        );
        if let Some(&(_, first_child)) = self.parents.last() {
            assert!(
                position >= first_child,
                "checkpoint is outside the current node"
            );
        }
        self.parents.push((kind, position));
    }

    /// Finishes building and returns the root node.
    ///
    /// # Panics
    ///
    /// Panics unless exactly one node was built and every started node was finished.
    #[must_use]
    pub fn finish(mut self) -> GreenNode {
        assert!(self.parents.is_empty(), "unfinished nodes");
        assert_eq!(self.children.len(), 1, "a tree has exactly one root");
        match self.children.pop() {
            Some(GreenElement::Node(node)) => node,
            _ => panic!("the root must be a node"),
        }
    }
}
