//! The red tree: cheap cursors over the green tree with parent pointers and absolute
//! offsets.

use std::fmt::{self, Write as _};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use loom_span::{TextRange, TextSize};

use crate::SyntaxKind;
use crate::green::{GreenElement, GreenNode, GreenToken};

/// A node in the CST, with its position and parent.
#[derive(Clone)]
pub struct SyntaxNode(Rc<NodeData>);

struct NodeData {
    green: GreenNode,
    parent: Option<SyntaxNode>,
    /// The node's index among its parent's children.
    index: usize,
    offset: TextSize,
}

impl SyntaxNode {
    /// Creates the root of a red tree.
    #[must_use]
    pub fn new_root(green: GreenNode) -> Self {
        Self(Rc::new(NodeData {
            green,
            parent: None,
            index: 0,
            offset: TextSize::new(0),
        }))
    }

    fn new_child(green: GreenNode, parent: Self, index: usize, offset: TextSize) -> Self {
        Self(Rc::new(NodeData {
            green,
            parent: Some(parent),
            index,
            offset,
        }))
    }

    /// The node's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.0.green.kind()
    }

    /// The underlying green node.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.0.green
    }

    /// The range of the node's text, including any trivia inside it.
    #[must_use]
    pub fn text_range(&self) -> TextRange {
        TextRange::at(self.0.offset, self.0.green.text_len())
    }

    /// The node's full text.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.0.green.write_text(&mut out);
        out
    }

    /// The parent node, or `None` for the root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.0.parent.clone()
    }

    /// This node and its ancestors, innermost first.
    pub fn ancestors(&self) -> impl Iterator<Item = Self> + use<> {
        std::iter::successors(Some(self.clone()), Self::parent)
    }

    /// Child nodes and tokens, in order.
    #[must_use]
    pub fn children_with_tokens(&self) -> SyntaxElementChildren {
        SyntaxElementChildren {
            parent: self.clone(),
            index: 0,
            offset: self.0.offset,
        }
    }

    /// Child nodes, in order.
    pub fn children(&self) -> impl Iterator<Item = Self> + use<> {
        self.children_with_tokens()
            .filter_map(SyntaxElement::into_node)
    }

    /// The first child node with the given kind.
    #[must_use]
    pub fn child_of_kind(&self, kind: SyntaxKind) -> Option<Self> {
        self.children().find(|child| child.kind() == kind)
    }

    /// The first child token with the given kind.
    #[must_use]
    pub fn token_of_kind(&self, kind: SyntaxKind) -> Option<SyntaxToken> {
        self.children_with_tokens()
            .filter_map(SyntaxElement::into_token)
            .find(|token| token.kind() == kind)
    }

    /// The first child node.
    #[must_use]
    pub fn first_child(&self) -> Option<Self> {
        self.children().next()
    }

    /// The child element at `index`, if any.
    fn child_at(&self, index: usize) -> Option<SyntaxElement> {
        let mut children = self.children_with_tokens();
        children.nth(index)
    }

    /// The next sibling node or token.
    #[must_use]
    pub fn next_sibling_or_token(&self) -> Option<SyntaxElement> {
        self.parent()?.child_at(self.0.index + 1)
    }

    /// The previous sibling node or token.
    #[must_use]
    pub fn prev_sibling_or_token(&self) -> Option<SyntaxElement> {
        let index = self.0.index.checked_sub(1)?;
        self.parent()?.child_at(index)
    }

    /// The first token in this subtree (including trivia).
    #[must_use]
    pub fn first_token(&self) -> Option<SyntaxToken> {
        self.children_with_tokens().find_map(|child| match child {
            SyntaxElement::Token(token) => Some(token),
            SyntaxElement::Node(node) => node.first_token(),
        })
    }

    /// The last token in this subtree (including trivia).
    #[must_use]
    pub fn last_token(&self) -> Option<SyntaxToken> {
        let children: Vec<_> = self.children_with_tokens().collect();
        children.into_iter().rev().find_map(|child| match child {
            SyntaxElement::Token(token) => Some(token),
            SyntaxElement::Node(node) => node.last_token(),
        })
    }

    /// This node and every node below it, in preorder.
    pub fn descendants(&self) -> impl Iterator<Item = Self> + use<> {
        self.preorder_with_tokens()
            .filter_map(SyntaxElement::into_node)
    }

    /// Every token in this subtree, in order.
    pub fn tokens(&self) -> impl Iterator<Item = SyntaxToken> + use<> {
        self.preorder_with_tokens()
            .filter_map(SyntaxElement::into_token)
    }

    /// This node and every node and token below it, in preorder.
    pub fn preorder_with_tokens(&self) -> impl Iterator<Item = SyntaxElement> + use<> {
        let mut stack = vec![SyntaxElement::Node(self.clone())];
        std::iter::from_fn(move || {
            let element = stack.pop()?;
            if let SyntaxElement::Node(node) = &element {
                let children: Vec<_> = node.children_with_tokens().collect();
                stack.extend(children.into_iter().rev());
            }
            Some(element)
        })
    }

    /// The token that covers `offset`, preferring the token that starts there.
    #[must_use]
    pub fn token_at_offset(&self, offset: TextSize) -> Option<SyntaxToken> {
        let range = self.text_range();
        if offset < range.start() || offset > range.end() {
            return None;
        }
        let mut node = self.clone();
        loop {
            let mut next = None;
            for child in node.children_with_tokens() {
                let child_range = child.text_range();
                if child_range.contains(offset)
                    || (child_range.end() == offset && offset == range.end())
                {
                    next = Some(child);
                    break;
                }
            }
            match next? {
                SyntaxElement::Token(token) => return Some(token),
                SyntaxElement::Node(child) => node = child,
            }
        }
    }

    /// An indented dump of the tree, one element per line: `KIND@start..end` for nodes
    /// and `KIND@start..end "text"` for tokens. Used by snapshot tests.
    #[must_use]
    pub fn debug_dump(&self) -> String {
        fn dump(node: &SyntaxNode, depth: usize, out: &mut String) {
            let indent = "  ".repeat(depth);
            let _ = writeln!(out, "{indent}{}@{}", node.kind(), node.text_range());
            for child in node.children_with_tokens() {
                match child {
                    SyntaxElement::Node(child) => dump(&child, depth + 1, out),
                    SyntaxElement::Token(token) => {
                        let _ = writeln!(
                            out,
                            "{indent}  {}@{} {:?}",
                            token.kind(),
                            token.text_range(),
                            token.text()
                        );
                    }
                }
            }
        }
        let mut out = String::new();
        dump(self, 0, &mut out);
        out
    }
}

impl PartialEq for SyntaxNode {
    fn eq(&self, other: &Self) -> bool {
        self.0.green.ptr_eq(&other.0.green) && self.0.offset == other.0.offset
    }
}

impl Eq for SyntaxNode {}

impl Hash for SyntaxNode {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.kind().hash(state);
        self.0.offset.hash(state);
        self.0.green.text_len().hash(state);
    }
}

impl fmt::Debug for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}@{}", self.kind(), self.text_range())
    }
}

impl fmt::Display for SyntaxNode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.text())
    }
}

/// A token in the CST, with its position and parent.
#[derive(Clone)]
pub struct SyntaxToken {
    parent: SyntaxNode,
    index: usize,
    offset: TextSize,
    green: GreenToken,
}

impl SyntaxToken {
    /// The token's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        self.green.kind()
    }

    /// The token's text.
    #[must_use]
    pub fn text(&self) -> &str {
        self.green.text()
    }

    /// The token's range.
    #[must_use]
    pub fn text_range(&self) -> TextRange {
        TextRange::at(self.offset, self.green.text_len())
    }

    /// The node that contains the token.
    #[must_use]
    pub fn parent(&self) -> SyntaxNode {
        self.parent.clone()
    }

    /// The underlying green token.
    #[must_use]
    pub fn green(&self) -> &GreenToken {
        &self.green
    }

    /// The next sibling node or token.
    #[must_use]
    pub fn next_sibling_or_token(&self) -> Option<SyntaxElement> {
        self.parent.child_at(self.index + 1)
    }

    /// The previous sibling node or token.
    #[must_use]
    pub fn prev_sibling_or_token(&self) -> Option<SyntaxElement> {
        let index = self.index.checked_sub(1)?;
        self.parent.child_at(index)
    }

    /// The next token in the whole tree (including trivia).
    #[must_use]
    pub fn next_token(&self) -> Option<Self> {
        let mut current = SyntaxElement::Token(self.clone());
        loop {
            match current.next_sibling_or_token() {
                Some(SyntaxElement::Token(token)) => return Some(token),
                Some(SyntaxElement::Node(node)) => match node.first_token() {
                    Some(token) => return Some(token),
                    None => current = SyntaxElement::Node(node),
                },
                None => current = SyntaxElement::Node(current.parent()?),
            }
        }
    }

    /// The previous token in the whole tree (including trivia).
    #[must_use]
    pub fn prev_token(&self) -> Option<Self> {
        let mut current = SyntaxElement::Token(self.clone());
        loop {
            match current.prev_sibling_or_token() {
                Some(SyntaxElement::Token(token)) => return Some(token),
                Some(SyntaxElement::Node(node)) => match node.last_token() {
                    Some(token) => return Some(token),
                    None => current = SyntaxElement::Node(node),
                },
                None => current = SyntaxElement::Node(current.parent()?),
            }
        }
    }
}

impl PartialEq for SyntaxToken {
    fn eq(&self, other: &Self) -> bool {
        self.parent == other.parent && self.index == other.index
    }
}

impl Eq for SyntaxToken {}

impl Hash for SyntaxToken {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parent.hash(state);
        self.index.hash(state);
    }
}

impl fmt::Debug for SyntaxToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}@{} {:?}",
            self.kind(),
            self.text_range(),
            self.text()
        )
    }
}

impl fmt::Display for SyntaxToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.text())
    }
}

/// A node or a token.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SyntaxElement {
    /// A node.
    Node(SyntaxNode),
    /// A token.
    Token(SyntaxToken),
}

impl SyntaxElement {
    /// The element's kind.
    #[must_use]
    pub fn kind(&self) -> SyntaxKind {
        match self {
            Self::Node(node) => node.kind(),
            Self::Token(token) => token.kind(),
        }
    }

    /// The element's range.
    #[must_use]
    pub fn text_range(&self) -> TextRange {
        match self {
            Self::Node(node) => node.text_range(),
            Self::Token(token) => token.text_range(),
        }
    }

    /// The parent node.
    #[must_use]
    pub fn parent(&self) -> Option<SyntaxNode> {
        match self {
            Self::Node(node) => node.parent(),
            Self::Token(token) => Some(token.parent()),
        }
    }

    /// The node, if this is a node.
    #[must_use]
    pub fn into_node(self) -> Option<SyntaxNode> {
        match self {
            Self::Node(node) => Some(node),
            Self::Token(_) => None,
        }
    }

    /// The token, if this is a token.
    #[must_use]
    pub fn into_token(self) -> Option<SyntaxToken> {
        match self {
            Self::Node(_) => None,
            Self::Token(token) => Some(token),
        }
    }

    /// The next sibling node or token.
    #[must_use]
    pub fn next_sibling_or_token(&self) -> Option<Self> {
        match self {
            Self::Node(node) => node.next_sibling_or_token(),
            Self::Token(token) => token.next_sibling_or_token(),
        }
    }

    /// The previous sibling node or token.
    #[must_use]
    pub fn prev_sibling_or_token(&self) -> Option<Self> {
        match self {
            Self::Node(node) => node.prev_sibling_or_token(),
            Self::Token(token) => token.prev_sibling_or_token(),
        }
    }
}

/// Iterator over the children of a node, created by
/// [`SyntaxNode::children_with_tokens`].
#[derive(Clone, Debug)]
pub struct SyntaxElementChildren {
    parent: SyntaxNode,
    index: usize,
    offset: TextSize,
}

impl Iterator for SyntaxElementChildren {
    type Item = SyntaxElement;

    fn next(&mut self) -> Option<SyntaxElement> {
        let green = self.parent.0.green.children().get(self.index)?.clone();
        let index = self.index;
        let offset = self.offset;
        self.index += 1;
        self.offset += green.text_len();
        Some(match green {
            GreenElement::Node(node) => SyntaxElement::Node(SyntaxNode::new_child(
                node,
                self.parent.clone(),
                index,
                offset,
            )),
            GreenElement::Token(token) => SyntaxElement::Token(SyntaxToken {
                parent: self.parent.clone(),
                index,
                offset,
                green: token,
            }),
        })
    }

    fn nth(&mut self, n: usize) -> Option<SyntaxElement> {
        // Skip without allocating red nodes, accumulating offsets from green lengths.
        let children = self.parent.0.green.children();
        for child in children.iter().skip(self.index).take(n) {
            self.offset += child.text_len();
        }
        self.index = (self.index + n).min(children.len());
        self.next()
    }
}
