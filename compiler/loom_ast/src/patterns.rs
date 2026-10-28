//! Patterns (spec §9).

use loom_syntax::{SyntaxKind, SyntaxToken};

use crate::{Name, Path, ast_enum, ast_node, child, children, token};

ast_enum!(
    /// Any pattern.
    Pattern {
        WildcardPat, IdentPat, LiteralPat, PathPat, TupleStructPat, TuplePat, ParenPat, RangePat,
        OrPat, RestPat,
    }
);

ast_node!(
    /// `_`.
    WildcardPat, WILDCARD_PAT
);

ast_node!(
    /// A binding, or a unit variant such as `None` (name resolution decides).
    IdentPat, IDENT_PAT
);

impl IdentPat {
    /// The name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }
}

ast_node!(
    /// A literal, optionally negated.
    LiteralPat, LITERAL_PAT
);

impl LiteralPat {
    /// Whether the literal has a leading `-`.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        token(&self.0, SyntaxKind::MINUS).is_some()
    }
}

ast_node!(
    /// A path such as `Shape.Circle`.
    PathPat, PATH_PAT
);

impl PathPat {
    /// The path.
    #[must_use]
    pub fn path(&self) -> Option<Path> {
        child(&self.0)
    }
}

ast_node!(
    /// `Path(fields)`.
    TupleStructPat, TUPLE_STRUCT_PAT
);

impl TupleStructPat {
    /// The path.
    #[must_use]
    pub fn path(&self) -> Option<Path> {
        child(&self.0)
    }

    /// Positional sub-patterns.
    pub fn patterns(&self) -> impl Iterator<Item = Pattern> + use<> {
        children(&self.0)
    }

    /// `name: pattern` fields.
    pub fn fields(&self) -> impl Iterator<Item = FieldPat> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `name: pattern` or the shorthand `name:`.
    FieldPat, FIELD_PAT
);

impl FieldPat {
    /// The field name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The sub-pattern; `None` for the shorthand.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }
}

ast_node!(
    /// `..` inside a tuple or variant pattern.
    RestPat, REST_PAT
);

ast_node!(
    /// `()` or `(a, b)`.
    TuplePat, TUPLE_PAT
);

impl TuplePat {
    /// The element patterns.
    pub fn elements(&self) -> impl Iterator<Item = Pattern> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `(pattern)`.
    ParenPat, PAREN_PAT
);

impl ParenPat {
    /// The inner pattern.
    #[must_use]
    pub fn pattern(&self) -> Option<Pattern> {
        child(&self.0)
    }
}

ast_node!(
    /// `a..b` or `a..=b`.
    RangePat, RANGE_PAT
);

impl RangePat {
    /// The bounds.
    pub fn bounds(&self) -> impl Iterator<Item = LiteralPat> + use<> {
        children(&self.0)
    }

    /// `..` or `..=`.
    #[must_use]
    pub fn operator(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::DOT2).or_else(|| token(&self.0, SyntaxKind::DOT2_EQ))
    }
}

ast_node!(
    /// `A | B`.
    OrPat, OR_PAT
);

impl OrPat {
    /// The alternatives.
    pub fn alternatives(&self) -> impl Iterator<Item = Pattern> + use<> {
        children(&self.0)
    }
}
