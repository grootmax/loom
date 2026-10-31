//! The high-level intermediate representation (ADR 0019 §1).
//!
//! HIR is a desugared, arena-based form of the AST. It has two parts:
//!
//! - the **item tree** ([`Hir::defs`]): every item of every file, with its name,
//!   visibility, attributes, generic parameters and *unresolved* signature
//!   ([`TypeRef`], [`Path`]);
//! - **bodies** ([`Body`]): per function or constant, arenas of [`Expr`], [`Pat`] and
//!   [`Binding`] addressed by `u32` indices, plus a map back to source ranges.
//!
//! Lowering ([`lower_file`]) is purely syntactic and total: it never fails and reports
//! nothing. A node the parser could not build becomes [`Expr::Missing`] or
//! [`Pat::Missing`]. Names are resolved by `loom_resolve` and types by `loom_check`.

mod lower;
mod pretty;

pub use lower::lower_file;
pub use pretty::dump_body;

use loom_span::{FileId, Span, TextRange};
use std::fmt;
use std::rc::Rc;

/// An identifier.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(Rc<str>);

impl Name {
    /// Creates a name.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self(Rc::from(text))
    }

    /// The name's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the first character is an uppercase letter (spec §9.1 rule 1).
    #[must_use]
    pub fn is_capitalized(&self) -> bool {
        self.0.chars().next().is_some_and(char::is_uppercase)
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u32);

        impl $name {
            /// The index as a `usize`.
            #[must_use]
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(
    /// An item in [`Hir::defs`].
    DefId
);
id!(
    /// A body in [`Hir::bodies`].
    BodyId
);
id!(
    /// An expression in a [`Body`].
    ExprId
);
id!(
    /// A pattern in a [`Body`].
    PatId
);
id!(
    /// A local binding in a [`Body`].
    BindingId
);

/// A package: the core library or the user's program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Package {
    /// The embedded core library.
    Core,
    /// The program being compiled.
    Main,
}

/// The whole program in HIR form.
#[derive(Debug, Default)]
pub struct Hir {
    /// Every item.
    pub defs: Vec<DefData>,
    /// Every body.
    pub bodies: Vec<Body>,
    /// The root module of each package.
    pub roots: Vec<(Package, DefId)>,
}

impl Hir {
    /// Creates an empty program.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The item `id`.
    #[must_use]
    pub fn def(&self, id: DefId) -> &DefData {
        &self.defs[id.index()]
    }

    /// The body `id`.
    #[must_use]
    pub fn body(&self, id: BodyId) -> &Body {
        &self.bodies[id.index()]
    }

    /// The root module of `package`.
    #[must_use]
    pub fn root(&self, package: Package) -> Option<DefId> {
        self.roots
            .iter()
            .find(|(p, _)| *p == package)
            .map(|&(_, id)| id)
    }

    /// The package that contains `id`.
    #[must_use]
    pub fn package_of(&self, id: DefId) -> Package {
        self.def(id).package
    }

    /// Every item, with its id.
    pub fn iter_defs(&self) -> impl Iterator<Item = (DefId, &DefData)> {
        self.defs
            .iter()
            .enumerate()
            .map(|(i, d)| (DefId(u32::try_from(i).unwrap_or(u32::MAX)), d))
    }

    /// The generic parameters declared by `id` itself (not by its parents).
    #[must_use]
    pub fn own_generics(&self, id: DefId) -> Option<&Generics> {
        match &self.def(id).kind {
            DefKind::Struct(s) => Some(&s.generics),
            DefKind::Enum(e) => Some(&e.generics),
            DefKind::Trait(t) => Some(&t.generics),
            DefKind::Impl(i) => Some(&i.generics),
            DefKind::Fn(f) => Some(&f.generics),
            DefKind::TypeAlias(t) => Some(&t.generics),
            _ => None,
        }
    }

    /// The item whose generic parameters are in scope inside `id` before its own: the
    /// enclosing struct, enum, trait or impl of a method.
    #[must_use]
    pub fn generics_parent(&self, id: DefId) -> Option<DefId> {
        let data = self.def(id);
        match data.kind {
            DefKind::Fn(_) | DefKind::Const(_) | DefKind::AssocType(_) | DefKind::Variant(_) => {
                data.parent.filter(|&p| {
                    matches!(
                        self.def(p).kind,
                        DefKind::Struct(_)
                            | DefKind::Enum(_)
                            | DefKind::Trait(_)
                            | DefKind::Impl(_)
                    )
                })
            }
            _ => None,
        }
    }

    /// A readable path for `id`, such as `geometry.Point.distance_to`.
    #[must_use]
    pub fn path_string(&self, id: DefId) -> String {
        let data = self.def(id);
        let mut parts = Vec::new();
        if !data.name.as_str().is_empty() {
            parts.push(data.name.to_string());
        }
        let mut current = data.parent;
        while let Some(parent) = current {
            let p = self.def(parent);
            if !p.name.as_str().is_empty() && !matches!(p.kind, DefKind::Impl(_)) {
                parts.push(p.name.to_string());
            }
            current = p.parent;
        }
        parts.reverse();
        parts.join(".")
    }
}

/// Visibility (spec §3.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    /// Visible in the declaring module and its descendants.
    Private,
    /// `pub(package)`.
    Package,
    /// `pub`.
    Public,
}

/// An attribute such as `@[derive(Copy, Eq)]`.
#[derive(Clone, Debug)]
pub struct Attr {
    /// The attribute's name.
    pub name: Name,
    /// Its arguments.
    pub args: Vec<AttrArg>,
    /// The attribute entry's source range.
    pub range: TextRange,
}

/// An attribute argument.
#[derive(Clone, Debug)]
pub enum AttrArg {
    /// A path, such as `Copy` in `derive(Copy)`.
    Path(Name, TextRange),
    /// A string literal.
    Str(String, TextRange),
    /// Anything else (kept for diagnostics).
    Other(TextRange),
}

/// An item.
#[derive(Clone, Debug)]
pub struct DefData {
    /// The declared name (empty for `impl`s and root modules).
    pub name: Name,
    /// What kind of item it is.
    pub kind: DefKind,
    /// The enclosing item, if any.
    pub parent: Option<DefId>,
    /// The nearest enclosing module (the item itself for a module).
    pub module: DefId,
    /// The package containing the item.
    pub package: Package,
    /// Declared visibility.
    pub vis: Visibility,
    /// Attributes.
    pub attrs: Vec<Attr>,
    /// Doc comment text, if any.
    pub doc: Option<String>,
    /// The name's span (or the item's, if it has no name).
    pub span: Span,
}

impl DefData {
    /// The attribute named `name`, if present.
    #[must_use]
    pub fn attr(&self, name: &str) -> Option<&Attr> {
        self.attrs.iter().find(|a| a.name.as_str() == name)
    }

    /// The `lang("…")` name of a core-library item.
    #[must_use]
    pub fn lang(&self) -> Option<&str> {
        self.attr("lang").and_then(|a| match a.args.first() {
            Some(AttrArg::Str(s, _)) => Some(s.as_str()),
            _ => None,
        })
    }

    /// The traits named in `@[derive(…)]`.
    #[must_use]
    pub fn derives(&self) -> Vec<(Name, TextRange)> {
        self.attrs
            .iter()
            .filter(|a| a.name.as_str() == "derive")
            .flat_map(|a| a.args.iter())
            .filter_map(|arg| match arg {
                AttrArg::Path(name, range) => Some((name.clone(), *range)),
                _ => None,
            })
            .collect()
    }
}

/// The kinds of items.
#[derive(Clone, Debug)]
pub enum DefKind {
    /// A module (a file or `module … end`).
    Module(ModuleData),
    /// A struct.
    Struct(StructData),
    /// An enum.
    Enum(EnumData),
    /// An enum variant.
    Variant(VariantData),
    /// A trait.
    Trait(TraitData),
    /// An `impl Trait for Type`.
    Impl(ImplData),
    /// A function, method or associated function.
    Fn(FnData),
    /// A constant.
    Const(ConstData),
    /// A type alias, or an associated type definition in an `impl`.
    TypeAlias(TypeAliasData),
    /// An associated type declaration in a trait.
    AssocType(AssocTypeData),
}

impl DefKind {
    /// A short description, for diagnostics.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Self::Module(_) => "module",
            Self::Struct(_) => "struct",
            Self::Enum(_) => "enum",
            Self::Variant(_) => "variant",
            Self::Trait(_) => "trait",
            Self::Impl(_) => "impl",
            Self::Fn(_) => "function",
            Self::Const(_) => "constant",
            Self::TypeAlias(_) => "type alias",
            Self::AssocType(_) => "associated type",
        }
    }
}

/// A module's contents.
#[derive(Clone, Debug)]
pub struct ModuleData {
    /// Items declared directly in the module (not `use`s, not members of items).
    pub items: Vec<DefId>,
    /// `use` declarations, flattened.
    pub uses: Vec<UseItem>,
    /// The file the module is in.
    pub file: FileId,
}

/// One imported name: `use a.b.c as d` or one leaf of `use a.{b, c}`.
#[derive(Clone, Debug)]
pub struct UseItem {
    /// The full path.
    pub path: Path,
    /// The name it is imported as.
    pub alias: Name,
    /// `pub use`.
    pub vis: Visibility,
    /// The source range of the leaf.
    pub range: TextRange,
}

/// Generic parameters and `where` predicates.
#[derive(Clone, Debug, Default)]
pub struct Generics {
    /// The parameters, in order.
    pub params: Vec<GenericParam>,
    /// `where` predicates: a type and its bounds.
    pub where_preds: Vec<(TypeRef, Vec<TypeRef>)>,
}

/// A generic parameter `T: Bound + Bound`.
#[derive(Clone, Debug)]
pub struct GenericParam {
    /// The name.
    pub name: Name,
    /// The bounds (trait paths).
    pub bounds: Vec<TypeRef>,
    /// The name's range.
    pub range: TextRange,
}

/// A struct.
#[derive(Clone, Debug, Default)]
pub struct StructData {
    /// Generic parameters.
    pub generics: Generics,
    /// Fields, in declaration order.
    pub fields: Vec<FieldData>,
    /// Inherent methods, associated functions and constants.
    pub members: Vec<DefId>,
}

/// A field of a struct or named variant.
#[derive(Clone, Debug)]
pub struct FieldData {
    /// The field name.
    pub name: Name,
    /// Its type.
    pub ty: TypeRef,
    /// Its visibility.
    pub vis: Visibility,
    /// The name's range.
    pub range: TextRange,
}

/// An enum.
#[derive(Clone, Debug, Default)]
pub struct EnumData {
    /// Generic parameters.
    pub generics: Generics,
    /// Variants, in declaration order.
    pub variants: Vec<DefId>,
    /// Inherent methods, associated functions and constants.
    pub members: Vec<DefId>,
}

/// An enum variant.
#[derive(Clone, Debug)]
pub struct VariantData {
    /// The position among the enum's variants.
    pub index: u32,
    /// The payload.
    pub fields: VariantFields,
}

/// A variant's payload.
#[derive(Clone, Debug)]
pub enum VariantFields {
    /// `Variant`.
    Unit,
    /// `Variant(A, B)`.
    Positional(Vec<TypeRef>),
    /// `Variant(a: A, b: B)`.
    Named(Vec<FieldData>),
}

/// A trait.
#[derive(Clone, Debug, Default)]
pub struct TraitData {
    /// Generic parameters.
    pub generics: Generics,
    /// Supertraits.
    pub supertraits: Vec<TypeRef>,
    /// Methods (required and provided), associated types and constants.
    pub members: Vec<DefId>,
}

/// An `impl Trait for Type`.
#[derive(Clone, Debug)]
pub struct ImplData {
    /// Generic parameters.
    pub generics: Generics,
    /// The implemented trait.
    pub trait_ref: TypeRef,
    /// The implementing type.
    pub self_ty: TypeRef,
    /// Methods, associated type definitions and constants.
    pub members: Vec<DefId>,
}

/// A parameter mode (spec §8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The default: a read-only borrow.
    Borrow,
    /// `mut`.
    Mut,
    /// `own`.
    Own,
}

/// A function, method or associated function.
#[derive(Clone, Debug)]
pub struct FnData {
    /// Generic parameters.
    pub generics: Generics,
    /// The receiver's mode, for methods. `None` for free and associated functions.
    pub receiver: Option<Mode>,
    /// Parameters, excluding the receiver.
    pub params: Vec<ParamData>,
    /// The declared return type; `None` means `Unit`.
    pub ret: Option<TypeRef>,
    /// The body (which also holds the default values of parameters).
    pub body: Option<BodyId>,
    /// `default def` in a trait.
    pub is_default: bool,
    /// The ABI of a function declared in an `extern "…"` block.
    pub extern_abi: Option<String>,
}

/// A function parameter.
#[derive(Clone, Debug)]
pub struct ParamData {
    /// Its mode.
    pub mode: Mode,
    /// Its name.
    pub name: Name,
    /// Its type.
    pub ty: TypeRef,
    /// Whether it has a default value (stored in the body's `defaults`).
    pub has_default: bool,
    /// The name's range.
    pub range: TextRange,
}

/// A constant.
#[derive(Clone, Debug)]
pub struct ConstData {
    /// Its type.
    pub ty: TypeRef,
    /// The body holding its value.
    pub body: Option<BodyId>,
}

/// A type alias (or an associated type definition in an `impl`).
#[derive(Clone, Debug)]
pub struct TypeAliasData {
    /// Generic parameters.
    pub generics: Generics,
    /// The aliased type.
    pub ty: TypeRef,
}

/// An associated type declaration in a trait.
#[derive(Clone, Debug)]
pub struct AssocTypeData {
    /// Bounds on the associated type.
    pub bounds: Vec<TypeRef>,
}

/// A path such as `std.io.Reader` or `Map[String, Int]`.
#[derive(Clone, Debug)]
pub struct Path {
    /// The segments.
    pub segments: Vec<PathSegment>,
    /// The whole path's range.
    pub range: TextRange,
}

impl Path {
    /// The path as `a.b.c`, without generic arguments.
    #[must_use]
    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

/// One segment of a path.
#[derive(Clone, Debug)]
pub struct PathSegment {
    /// The identifier (or `self`, `Self`, `super`, `package`).
    pub name: Name,
    /// Generic arguments given to this segment.
    pub args: Vec<TypeRef>,
    /// The segment's range.
    pub range: TextRange,
}

/// A type as written in the source.
#[derive(Clone, Debug)]
pub struct TypeRef {
    /// The form of the type.
    pub kind: TypeRefKind,
    /// Its range.
    pub range: TextRange,
}

/// The forms of a written type.
#[derive(Clone, Debug)]
pub enum TypeRefKind {
    /// A named type.
    Path(Path),
    /// A tuple type; `()` is `Unit`.
    Tuple(Vec<TypeRef>),
    /// `Fn(A) -> R`, `FnMut(…)`, `FnOnce(…)`.
    Fn {
        /// Which callable kind.
        kind: FnKind,
        /// Parameter types.
        params: Vec<TypeRef>,
        /// The return type (`None` is `Unit`).
        ret: Option<Box<TypeRef>>,
    },
    /// `dyn A + B`.
    Dyn(Vec<TypeRef>),
    /// `ref T` or `mut ref T`.
    Ref {
        /// `mut ref`.
        mutable: bool,
        /// The referenced type.
        inner: Box<TypeRef>,
    },
    /// `Self`.
    SelfType,
    /// A type the parser could not build.
    Error,
}

/// The callable kinds (spec §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FnKind {
    /// `Fn`.
    Fn,
    /// `FnMut`.
    FnMut,
    /// `FnOnce`.
    FnOnce,
}

/// A function or constant body.
#[derive(Clone, Debug)]
pub struct Body {
    /// The item that owns the body.
    pub owner: DefId,
    /// The file the body is in.
    pub file: FileId,
    /// Expressions.
    pub exprs: Vec<Expr>,
    /// Source ranges of expressions.
    pub expr_ranges: Vec<TextRange>,
    /// Patterns.
    pub pats: Vec<Pat>,
    /// Source ranges of patterns.
    pub pat_ranges: Vec<TextRange>,
    /// Local bindings.
    pub bindings: Vec<Binding>,
    /// One binding per parameter, in order (excluding the receiver).
    pub params: Vec<BindingId>,
    /// Default values of parameters, in order.
    pub defaults: Vec<Option<ExprId>>,
    /// The body expression (a block, or the single expression of `def f = e`).
    pub root: Option<ExprId>,
}

impl Body {
    /// The expression `id`.
    #[must_use]
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.index()]
    }

    /// The pattern `id`.
    #[must_use]
    pub fn pat(&self, id: PatId) -> &Pat {
        &self.pats[id.index()]
    }

    /// The binding `id`.
    #[must_use]
    pub fn binding(&self, id: BindingId) -> &Binding {
        &self.bindings[id.index()]
    }

    /// The source range of `id`.
    #[must_use]
    pub fn expr_range(&self, id: ExprId) -> TextRange {
        self.expr_ranges[id.index()]
    }

    /// The source range of `id`.
    #[must_use]
    pub fn pat_range(&self, id: PatId) -> TextRange {
        self.pat_ranges[id.index()]
    }

    /// The span of `id`.
    #[must_use]
    pub fn expr_span(&self, id: ExprId) -> Span {
        Span::new(self.file, self.expr_range(id))
    }

    /// The span of `id`.
    #[must_use]
    pub fn pat_span(&self, id: PatId) -> Span {
        Span::new(self.file, self.pat_range(id))
    }

    /// Every expression id.
    pub fn expr_ids(&self) -> impl Iterator<Item = ExprId> + use<> {
        (0..u32::try_from(self.exprs.len()).unwrap_or(u32::MAX)).map(ExprId)
    }
}

/// A local binding: a parameter, a `x = …`/`var x = …` statement, or a pattern binding.
#[derive(Clone, Debug)]
pub struct Binding {
    /// The bound name.
    pub name: Name,
    /// The binding's mutability and origin.
    pub kind: BindingKind,
    /// The name's range.
    pub range: TextRange,
}

/// Where a binding comes from, which decides whether it can be assigned or mutated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    /// `x = …`.
    Let,
    /// `var x = …`.
    Var,
    /// A function parameter with this mode.
    Param(Mode),
    /// A closure parameter with this mode.
    ClosureParam(Mode),
    /// A binding in a pattern.
    Pattern,
}

impl BindingKind {
    /// Whether the binding is a mutable place (spec §4.2 rule 1).
    #[must_use]
    pub fn is_mutable(self) -> bool {
        matches!(
            self,
            Self::Var | Self::Param(Mode::Mut) | Self::ClosureParam(Mode::Mut)
        )
    }
}

/// A literal value.
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    /// An integer, before its type is known.
    Int(u128),
    /// A float.
    Float(f64),
    /// A character.
    Char(char),
    /// A string without interpolation.
    String(Rc<str>),
    /// A byte string.
    Bytes(Rc<[u8]>),
    /// `true` or `false`.
    Bool(bool),
}

/// A part of an interpolated string.
#[derive(Clone, Debug)]
pub enum InterpolationPart {
    /// Literal text.
    Text(Rc<str>),
    /// `#{expr}`.
    Expr(ExprId),
}

/// An argument of a call.
#[derive(Clone, Debug)]
pub struct Arg {
    /// The parameter name, for named arguments.
    pub name: Option<(Name, TextRange)>,
    /// The value.
    pub value: ExprId,
    /// Whether this is the trailing block.
    pub trailing: bool,
}

/// Binary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Rem,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
    /// `^`.
    BitXor,
    /// `<<`.
    Shl,
    /// `>>`.
    Shr,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `and`.
    And,
    /// `or`.
    Or,
}

impl BinaryOp {
    /// The operator as written.
    #[must_use]
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "^",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
            Self::And => "and",
            Self::Or => "or",
        }
    }

    /// `+ - * / %`.
    #[must_use]
    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Rem
        )
    }

    /// `& | ^`.
    #[must_use]
    pub fn is_bitwise(self) -> bool {
        matches!(self, Self::BitAnd | Self::BitOr | Self::BitXor)
    }

    /// `<< >>`.
    #[must_use]
    pub fn is_shift(self) -> bool {
        matches!(self, Self::Shl | Self::Shr)
    }

    /// `== !=`.
    #[must_use]
    pub fn is_equality(self) -> bool {
        matches!(self, Self::Eq | Self::Ne)
    }

    /// `< <= > >=`.
    #[must_use]
    pub fn is_ordering(self) -> bool {
        matches!(self, Self::Lt | Self::Le | Self::Gt | Self::Ge)
    }
}

/// Unary operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-`.
    Neg,
    /// `~`.
    BitNot,
    /// `not`.
    Not,
}

/// A statement in a block.
#[derive(Clone, Debug)]
pub enum Stmt {
    /// `x = e`, `var x: T = e`. Resolution may reclassify a non-`var` one as an
    /// assignment (spec §4.1).
    Bind {
        /// The binding this statement introduces (if it is not an assignment).
        binding: BindingId,
        /// The annotated type.
        ty: Option<TypeRef>,
        /// The value.
        value: ExprId,
    },
    /// `place = e` or `place op= e`.
    Assign {
        /// The assigned place.
        place: ExprId,
        /// The operator of a compound assignment.
        op: Option<BinaryOp>,
        /// The value.
        value: ExprId,
        /// The operator's range.
        op_range: TextRange,
    },
    /// An expression statement.
    Expr(ExprId),
}

/// An arm of a `case`.
#[derive(Clone, Debug)]
pub struct CaseArm {
    /// The pattern.
    pub pat: PatId,
    /// The guard (`if cond`).
    pub guard: Option<ExprId>,
    /// The arm's value.
    pub body: ExprId,
}

/// An arm of `select`: `when [pat =] operation …`.
#[derive(Clone, Debug)]
pub struct SelectArm {
    /// The pattern a received value is matched against.
    pub pat: Option<PatId>,
    /// The operation: `ch.recv`, `ch.send(v)` or `Time.after(d)`.
    pub operation: ExprId,
    /// The arm's body.
    pub body: ExprId,
}

/// A closure parameter.
#[derive(Clone, Debug)]
pub struct ClosureParam {
    /// Its mode.
    pub mode: Mode,
    /// Its pattern.
    pub pat: PatId,
    /// Its annotated type.
    pub ty: Option<TypeRef>,
}

/// An expression.
#[derive(Clone, Debug)]
pub enum Expr {
    /// An expression the parser could not build.
    Missing,
    /// A literal.
    Literal(Literal),
    /// An interpolated string.
    Interpolate(Vec<InterpolationPart>),
    /// A single identifier (a local, an item, a type, a module, …).
    Name(Name),
    /// `self` in a method.
    SelfValue,
    /// `Self` used as a value (for example `Self.new()`).
    SelfType,
    /// `base.name`: a field, a zero-argument method call, or the next segment of a path.
    Field {
        /// The receiver or path prefix.
        base: ExprId,
        /// The field or member name.
        name: Name,
        /// The name's range.
        name_range: TextRange,
    },
    /// `base.0`.
    TupleField {
        /// The tuple.
        base: ExprId,
        /// The element index.
        index: u32,
    },
    /// `callee(args)`.
    Call {
        /// The called expression.
        callee: ExprId,
        /// The arguments, with a trailing block last.
        args: Vec<Arg>,
    },
    /// `receiver.name(args)` or `receiver.name { block }`.
    MethodCall {
        /// The receiver.
        receiver: ExprId,
        /// The method name.
        name: Name,
        /// The name's range.
        name_range: TextRange,
        /// The arguments, with a trailing block last.
        args: Vec<Arg>,
    },
    /// `base[indices]`: indexing, or generic arguments when `base` names a type.
    Index {
        /// The indexed expression.
        base: ExprId,
        /// The index expressions.
        indices: Vec<ExprId>,
        /// The indices read as types, when they all have the shape of types.
        type_args: Option<Vec<TypeRef>>,
    },
    /// A unary operation.
    Unary {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: ExprId,
    },
    /// A binary operation.
    Binary {
        /// The operator.
        op: BinaryOp,
        /// The left operand.
        lhs: ExprId,
        /// The right operand.
        rhs: ExprId,
        /// The operator's range.
        op_range: TextRange,
    },
    /// `a..b` or `a..=b`.
    Range {
        /// The start.
        start: ExprId,
        /// The end.
        end: ExprId,
        /// `..=`.
        inclusive: bool,
    },
    /// `expr in pattern`.
    In {
        /// The tested value.
        expr: ExprId,
        /// The pattern.
        pat: PatId,
    },
    /// `expr?`.
    Try(ExprId),
    /// `(a, b)`; `()` is the unit value.
    Tuple(Vec<ExprId>),
    /// `[a, b]`.
    Array(Vec<ExprId>),
    /// `{k => v}`.
    Map(Vec<(ExprId, ExprId)>),
    /// A closure.
    Closure {
        /// Parameters; `None` when the block has no `|…|`.
        params: Option<Vec<ClosureParam>>,
        /// The body.
        body: ExprId,
    },
    /// A block of statements.
    Block {
        /// The statements; the value is that of the last one if it is an expression.
        stmts: Vec<Stmt>,
    },
    /// `if`.
    If {
        /// The condition.
        cond: ExprId,
        /// The `then` branch.
        then_branch: ExprId,
        /// The `else` branch (an `elsif` becomes a nested `if`).
        else_branch: Option<ExprId>,
    },
    /// `case`.
    Case {
        /// The scrutinee.
        scrutinee: ExprId,
        /// The arms.
        arms: Vec<CaseArm>,
        /// The `else` branch.
        else_branch: Option<ExprId>,
    },
    /// `select … end` (spec §11.3).
    Select {
        /// The arms.
        arms: Vec<SelectArm>,
        /// The `else` arm.
        else_branch: Option<ExprId>,
    },
    /// `loop do … end`.
    Loop(ExprId),
    /// `while cond … end`.
    While {
        /// The condition.
        cond: ExprId,
        /// The body.
        body: ExprId,
    },
    /// `for pat in iter … end`.
    For {
        /// The pattern.
        pat: PatId,
        /// The iterated expression.
        iter: ExprId,
        /// The body.
        body: ExprId,
    },
    /// `return`.
    Return(Option<ExprId>),
    /// `break`.
    Break(Option<ExprId>),
    /// `next`.
    Next,
    /// A construct this phase does not implement (`select`, `unsafe`).
    Unsupported(&'static str),
}

/// A pattern.
#[derive(Clone, Debug)]
pub enum Pat {
    /// A pattern the parser could not build.
    Missing,
    /// `_`.
    Wild,
    /// A binding.
    Bind(BindingId),
    /// A literal (with an optional leading `-`).
    Literal {
        /// The literal.
        lit: Literal,
        /// A leading `-`.
        negative: bool,
    },
    /// `a..b` or `a..=b`.
    Range {
        /// The lower bound.
        lo: (Literal, bool),
        /// The upper bound.
        hi: (Literal, bool),
        /// `..=`.
        inclusive: bool,
    },
    /// `(a, b)`.
    Tuple(Vec<PatId>),
    /// A capitalized path: a unit variant, unit struct or constant.
    Path(Path),
    /// `Path(p, q, ..)`.
    TupleStruct {
        /// The variant or struct path.
        path: Path,
        /// The element patterns.
        pats: Vec<PatId>,
        /// Whether `..` was present.
        rest: bool,
    },
    /// `Path(f: p, g:, ..)`.
    Struct {
        /// The variant or struct path.
        path: Path,
        /// The field patterns.
        fields: Vec<(Name, TextRange, PatId)>,
        /// Whether `..` was present.
        rest: bool,
    },
    /// `p | q`.
    Or(Vec<PatId>),
    /// `..` in a tuple pattern (only valid there).
    Rest,
}
