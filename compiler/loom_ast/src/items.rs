//! Items (spec §3).

use loom_syntax::{SyntaxKind, SyntaxToken};

use crate::{
    AstNode, Expr, Name, Path, PathType, Type, ast_enum, ast_node, child, children,
    first_significant_token, token,
};

ast_enum!(
    /// Any item.
    Item {
        FnDef, StructDef, EnumDef, TraitDef, ImplDef, ModuleDef, UseDecl, ConstDef, TypeAlias,
        ExternBlock,
    }
);

impl Item {
    /// The item's attributes.
    pub fn attributes(&self) -> impl Iterator<Item = Attribute> + use<> {
        children(self.syntax())
    }

    /// The item's visibility, if it has one.
    #[must_use]
    pub fn visibility(&self) -> Option<Visibility> {
        child(self.syntax())
    }

    /// The doc comment lines attached to the item, without the leading `##`.
    #[must_use]
    pub fn doc_comment(&self) -> Option<String> {
        doc_comment(self.syntax())
    }
}

/// Collects `##` lines attached to a node into one Markdown string.
pub(crate) fn doc_comment(node: &loom_syntax::SyntaxNode) -> Option<String> {
    let lines: Vec<String> = node
        .children_with_tokens()
        .filter_map(loom_syntax::SyntaxElement::into_token)
        .take_while(|t| t.kind().is_trivia())
        .filter(|t| t.kind() == SyntaxKind::DOC_COMMENT)
        .map(|t| {
            let text = t.text().trim_start_matches("##");
            text.strip_prefix(' ').unwrap_or(text).to_owned()
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join("\n"))
}

ast_node!(
    /// `@[entry, …]`.
    Attribute, ATTRIBUTE
);

impl Attribute {
    /// The entries, such as `derive(Eq)` and `test`.
    pub fn entries(&self) -> impl Iterator<Item = AttrEntry> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `name` or `name(args)` inside an attribute.
    AttrEntry, ATTR_ENTRY
);

impl AttrEntry {
    /// The entry's name.
    #[must_use]
    pub fn name(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::IDENT)
    }

    /// The arguments, if any.
    pub fn args(&self) -> impl Iterator<Item = AttrArg> + use<> {
        self.0
            .child_of_kind(SyntaxKind::ATTR_ARG_LIST)
            .into_iter()
            .flat_map(|list| children(&list).collect::<Vec<_>>())
    }
}

ast_node!(
    /// One attribute argument: a path, a literal, or `name: literal`.
    AttrArg, ATTR_ARG
);

impl AttrArg {
    /// The label, for `name: value` arguments.
    #[must_use]
    pub fn label(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::COLON).and(token(&self.0, SyntaxKind::IDENT))
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `pub` or `pub(package)`.
    Visibility, VISIBILITY
);

impl Visibility {
    /// Whether this is `pub(package)`.
    #[must_use]
    pub fn is_package(&self) -> bool {
        token(&self.0, SyntaxKind::PACKAGE_KW).is_some()
    }
}

ast_node!(
    /// `def …`: a function, method, associated function or trait method signature.
    FnDef, FN_DEF
);

impl FnDef {
    /// The function's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The receiver mode keyword of a method: `mut` or `own`.
    #[must_use]
    pub fn receiver_mode(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::MUT_KW).or_else(|| token(&self.0, SyntaxKind::OWN_KW))
    }

    /// Whether this is an associated function, `def self.name`.
    #[must_use]
    pub fn is_associated(&self) -> bool {
        token(&self.0, SyntaxKind::SELF_KW).is_some()
    }

    /// Whether this is a provided trait method, `default def`.
    #[must_use]
    pub fn is_default(&self) -> bool {
        first_significant_token(&self.0).is_some_and(|t| t.text() == "default")
    }

    /// The generic parameters.
    #[must_use]
    pub fn generic_params(&self) -> Option<GenericParamList> {
        child(&self.0)
    }

    /// The parameter list, if written.
    #[must_use]
    pub fn param_list(&self) -> Option<ParamList> {
        child(&self.0)
    }

    /// The return type.
    #[must_use]
    pub fn ret_type(&self) -> Option<Type> {
        self.0
            .child_of_kind(SyntaxKind::RET_TYPE)
            .and_then(|ret| child(&ret))
    }

    /// The `where` clause.
    #[must_use]
    pub fn where_clause(&self) -> Option<WhereClause> {
        child(&self.0)
    }

    /// The body of a single-expression function: `def f = expr`.
    #[must_use]
    pub fn expr_body(&self) -> Option<Expr> {
        token(&self.0, SyntaxKind::EQ)?;
        child(&self.0)
    }

    /// The block body of a multi-line function.
    #[must_use]
    pub fn block_body(&self) -> Option<crate::Block> {
        child(&self.0)
    }
}

ast_node!(
    /// `(param, …)`.
    ParamList, PARAM_LIST
);

impl ParamList {
    /// The parameters.
    pub fn params(&self) -> impl Iterator<Item = Param> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `[mut | own] name: Type [= default]`.
    Param, PARAM
);

impl Param {
    /// The parameter mode keyword: `mut` or `own` (borrow if absent).
    #[must_use]
    pub fn mode(&self) -> Option<SyntaxToken> {
        token(&self.0, SyntaxKind::MUT_KW).or_else(|| token(&self.0, SyntaxKind::OWN_KW))
    }

    /// The parameter's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The parameter's type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The default value.
    #[must_use]
    pub fn default_value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `[T: Bound, …]`.
    GenericParamList, GENERIC_PARAM_LIST
);

impl GenericParamList {
    /// The parameters.
    pub fn params(&self) -> impl Iterator<Item = GenericParam> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `T` or `T: Bound + Bound`.
    GenericParam, GENERIC_PARAM
);

impl GenericParam {
    /// The parameter's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The bounds.
    #[must_use]
    pub fn bounds(&self) -> Option<BoundList> {
        child(&self.0)
    }
}

ast_node!(
    /// `Bound + Bound`.
    BoundList, BOUND_LIST
);

impl BoundList {
    /// The bounds.
    pub fn bounds(&self) -> impl Iterator<Item = PathType> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `where T: Bound, …`.
    WhereClause, WHERE_CLAUSE
);

impl WhereClause {
    /// The predicates.
    pub fn predicates(&self) -> impl Iterator<Item = WherePred> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `Type: Bounds` in a `where` clause.
    WherePred, WHERE_PRED
);

impl WherePred {
    /// The constrained type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The bounds.
    #[must_use]
    pub fn bounds(&self) -> Option<BoundList> {
        child(&self.0)
    }
}

ast_node!(
    /// `struct Name … end`.
    StructDef, STRUCT_DEF
);

impl StructDef {
    /// The struct's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The generic parameters.
    #[must_use]
    pub fn generic_params(&self) -> Option<GenericParamList> {
        child(&self.0)
    }

    /// The fields.
    pub fn fields(&self) -> impl Iterator<Item = FieldDef> + use<> {
        children(&self.0)
    }

    /// The inherent methods.
    pub fn methods(&self) -> impl Iterator<Item = FnDef> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `name: Type`, in a struct or a struct-like variant.
    FieldDef, FIELD_DEF
);

impl FieldDef {
    /// The field's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The field's type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The field's visibility.
    #[must_use]
    pub fn visibility(&self) -> Option<Visibility> {
        child(&self.0)
    }
}

ast_node!(
    /// `enum Name … end`.
    EnumDef, ENUM_DEF
);

impl EnumDef {
    /// The enum's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The variants.
    pub fn variants(&self) -> impl Iterator<Item = VariantDef> + use<> {
        children(&self.0)
    }

    /// The inherent methods.
    pub fn methods(&self) -> impl Iterator<Item = FnDef> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `Variant`, `Variant(Type, …)` or `Variant(field: Type, …)`.
    VariantDef, VARIANT_DEF
);

impl VariantDef {
    /// The variant's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// Named fields of a struct-like variant.
    pub fn fields(&self) -> impl Iterator<Item = FieldDef> + use<> {
        self.0
            .child_of_kind(SyntaxKind::VARIANT_FIELD_LIST)
            .into_iter()
            .flat_map(|list| children(&list).collect::<Vec<_>>())
    }

    /// Types of a tuple-like variant.
    pub fn types(&self) -> impl Iterator<Item = Type> + use<> {
        self.0
            .child_of_kind(SyntaxKind::VARIANT_FIELD_LIST)
            .into_iter()
            .flat_map(|list| children(&list).collect::<Vec<_>>())
    }
}

ast_node!(
    /// `trait Name … end`.
    TraitDef, TRAIT_DEF
);

impl TraitDef {
    /// The trait's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The supertraits.
    #[must_use]
    pub fn supertraits(&self) -> Option<BoundList> {
        child(&self.0)
    }

    /// Required and provided methods.
    pub fn methods(&self) -> impl Iterator<Item = FnDef> + use<> {
        children(&self.0)
    }

    /// Associated types.
    pub fn assoc_types(&self) -> impl Iterator<Item = AssocType> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `type Item [: Bounds]` in a trait.
    AssocType, ASSOC_TYPE
);

impl AssocType {
    /// The associated type's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }
}

ast_node!(
    /// `impl Trait for Type … end`.
    ImplDef, IMPL_DEF
);

impl ImplDef {
    /// The implemented trait.
    #[must_use]
    pub fn trait_(&self) -> Option<PathType> {
        child(&self.0)
    }

    /// The type the trait is implemented for.
    #[must_use]
    pub fn self_type(&self) -> Option<Type> {
        token(&self.0, SyntaxKind::FOR_KW)?;
        children::<Type>(&self.0).nth(1)
    }

    /// The methods.
    pub fn methods(&self) -> impl Iterator<Item = FnDef> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `module name … end`.
    ModuleDef, MODULE_DEF
);

impl ModuleDef {
    /// The module's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The module's items.
    pub fn items(&self) -> impl Iterator<Item = Item> + use<> {
        children(&self.0)
    }
}

ast_node!(
    /// `use tree`.
    UseDecl, USE_DECL
);

impl UseDecl {
    /// The imported tree.
    #[must_use]
    pub fn tree(&self) -> Option<UseTree> {
        child(&self.0)
    }
}

ast_node!(
    /// `path`, `path as alias` or `path.{tree, …}`.
    UseTree, USE_TREE
);

impl UseTree {
    /// The path.
    #[must_use]
    pub fn path(&self) -> Option<Path> {
        child(&self.0)
    }

    /// The alias after `as`.
    #[must_use]
    pub fn alias(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The nested trees of `path.{…}`.
    pub fn subtrees(&self) -> impl Iterator<Item = UseTree> + use<> {
        self.0
            .child_of_kind(SyntaxKind::USE_TREE_LIST)
            .into_iter()
            .flat_map(|list| children(&list).collect::<Vec<_>>())
    }
}

ast_node!(
    /// `const NAME: Type = expr`.
    ConstDef, CONST_DEF
);

impl ConstDef {
    /// The constant's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The constant's type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }

    /// The value.
    #[must_use]
    pub fn value(&self) -> Option<Expr> {
        child(&self.0)
    }
}

ast_node!(
    /// `type Name = Type`.
    TypeAlias, TYPE_ALIAS
);

impl TypeAlias {
    /// The alias's name.
    #[must_use]
    pub fn name(&self) -> Option<Name> {
        child(&self.0)
    }

    /// The aliased type.
    #[must_use]
    pub fn ty(&self) -> Option<Type> {
        child(&self.0)
    }
}

ast_node!(
    /// `extern "ABI" … end`.
    ExternBlock, EXTERN_BLOCK
);

impl ExternBlock {
    /// The declared functions.
    pub fn functions(&self) -> impl Iterator<Item = FnDef> + use<> {
        children(&self.0)
    }
}
