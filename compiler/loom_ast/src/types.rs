//! Types and paths (spec §5, §7.2).

use loom_syntax::{SyntaxKind, SyntaxToken};

use crate::{BoundList, NameRef, ast_enum, ast_node, child, children, token};

ast_enum!(
    /// Any type.
    Type { PathType, TupleType, FnType, DynType, RefType }
);

ast_node!(
    /// `a.b.C`.
    Path, PATH
);

impl Path {
    /// The segments, in order.
    pub fn segments(&self) -> impl Iterator<Item = PathSegment> + use<> {
        children(&self.0)
    }

    /// The path as written, segments joined with `.`.
    #[must_use]
    pub fn text(&self) -> String {
        self.segments()
            .map(|segment| segment.name().map(|n| n.text()).unwrap_or_default())
            .collect::<Vec<_>>()
            .join(".")
    }
}

ast_node!(
    /// One segment of a path.
    PathSegment, PATH_SEGMENT
);

impl PathSegment {
    /// The segment's name.
    #[must_use]
    pub fn name(&self) -> Option<NameRef> {
        child(&self.0)
    }
}

ast_node!(
    /// `Path[Args]`.
    PathType, PATH_TYPE
);

impl PathType {
    /// The path.
    #[must_use]
    pub fn path(&self) -> Option<Path> {
        child(&self.0)
    }

    /// The generic arguments.
    pub fn generic_args(&self) -> impl Iterator<Item = Type> + use<> {
        self.0
            .child_of_kind(SyntaxKind::GENERIC_ARG_LIST)
            .into_iter()
            .flat_map(|list| children(&list).collect::<Vec<_>>())
    }
}

ast_node!(
    /// `()` or `(A, B)`.
    TupleType, TUPLE_TYPE
);

impl TupleType {
    /// The element types.
    pub fn elements(&self) -> impl Iterator<Item = Type> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `Fn(A) -> B`, `FnMut(…)`, `FnOnce(…)`.
    FnType, FN_TYPE
);

impl FnType {
    /// `Fn`, `FnMut` or `FnOnce`.
    #[must_use]
    pub fn kind_token(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::IDENT)
    }

    /// Parameter types, then the return type if there is an `->`.
    pub fn types(&self) -> impl Iterator<Item = Type> + use<> {
        children(&self.0)
    }

    /// The return type.
    #[must_use]
    pub fn ret_type(&self) -> Option<Type> {
        token(&self.0, SyntaxKind::ARROW)?;
        children(&self.0).last()
    }
}

ast_node!(
    /// `dyn Trait + Trait`.
    DynType, DYN_TYPE
);

impl DynType {
    /// The traits.
    #[must_use]
    pub fn bounds(&self) -> Option<BoundList> {
        child(&self.0)
    }
}

ast_node!(
    /// `ref T` or `mut ref T`.
    RefType, REF_TYPE
);

impl RefType {
    /// Whether this is `mut ref`.
    #[must_use]
    pub fn is_mut(&self) -> bool {
        token(&self.0, SyntaxKind::MUT_KW).is_some()
    }

    /// The referenced type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }
}
