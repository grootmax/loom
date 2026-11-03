//! Type inference and checking (ADR 0019 §3–§7, spec §5–§10).
//!
//! [`check`] lowers every signature ([`items`]), checks item-level rules (impls, derives,
//! coherence), then infers each body one at a time. The result, [`Checked`],
//! gives for each body the type of every expression, pattern and binding and the meaning
//! of every call, field access and path, which is all the interpreter needs.

pub mod display;
mod exhaustive;
mod infer;
mod item_checks;
pub mod items;
pub mod lower;
pub mod traits;

pub use items::{FnSig, ImplId, ImplInfo, ImplKind, Items, Predicate, TraitRef, VariantTy};
pub use traits::{Env, SelectError, Selection, Solver};

use loom_diagnostics::Diagnostic;
use loom_hir::{BindingId, BodyId, DefId, ExprId, FnKind, Hir, PatId};
use loom_resolve::{DefMap, Resolution};
use loom_types::{Interner, Subst, Ty};
use std::collections::HashMap;

/// What a call invokes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallTarget {
    /// A known function: a free function, an associated function, an inherent method,
    /// a trait method body or an intrinsic.
    Fn {
        /// The function.
        def: DefId,
        /// Its generic arguments (the parent's and its own).
        subst: Subst,
    },
    /// A trait method, dispatched on the concrete type of `self_ty` at run time.
    Trait {
        /// The trait's method declaration.
        method: DefId,
        /// The type implementing the trait.
        self_ty: Ty,
        /// The trait's and the method's generic arguments.
        subst: Subst,
    },
    /// A struct constructor.
    Struct {
        /// The struct.
        def: DefId,
        /// Its generic arguments.
        subst: Subst,
    },
    /// An enum variant constructor.
    Variant {
        /// The variant.
        def: DefId,
        /// The enum's generic arguments.
        subst: Subst,
    },
    /// A value of a callable type.
    Value,
}

/// Where the value of one parameter comes from at a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgSource {
    /// The argument at this index of the call's argument list.
    Arg(usize),
    /// The parameter's default value.
    Default,
}

/// How a `for` loop gets its iterator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForKind {
    /// The iterated value is an iterator.
    Iterator,
    /// The iterated value is `Iterable`; its `iter` method is called first.
    Iterable,
}

/// What an expression means, where its form alone does not say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprInfo {
    /// A local binding.
    Local(BindingId),
    /// A function used as a value.
    FnValue {
        /// The function.
        def: DefId,
        /// Its generic arguments.
        subst: Subst,
    },
    /// A constant.
    Const {
        /// The constant.
        def: DefId,
    },
    /// A unit variant value.
    UnitVariant {
        /// The variant.
        def: DefId,
    },
    /// A struct or named-variant field read, by position.
    Field(usize),
    /// A call, or a method call without arguments written as a field access.
    Call {
        /// What is called.
        target: CallTarget,
        /// The receiver, for method calls (an expression of the call).
        receiver: Option<ExprId>,
        /// One entry per parameter (excluding the receiver).
        args: Vec<ArgSource>,
    },
    /// `?` on a `Result` or an `Option`.
    Try {
        /// `true` for `Result`.
        result: bool,
        /// The `From.from` conversion of the error, when the error types differ.
        convert: Option<CallTarget>,
    },
    /// A `for` loop.
    For(ForKind),
    /// A closure of this kind.
    Closure(FnKind),
}

/// An implicit conversion applied to an expression's value (spec §6.5, §8.3 rule 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coercion {
    /// A `ref T` used as a `T`.
    Deref,
    /// A place used where a `ref T` is expected: it is borrowed.
    Autoref,
    /// A value converted to `dyn Trait` (the target type).
    Unsize(Ty),
    /// A method receiver reached through `Deref`: `deref` is called on the value and the
    /// method is called on its `Target` (spec §7.4 rule 6).
    Overloaded {
        /// The receiver's type.
        from: Ty,
        /// `Deref.Target` of that type.
        target: Ty,
    },
}

/// What a path pattern refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatInfo {
    /// An enum variant.
    Variant(DefId),
    /// A struct.
    Struct(DefId),
    /// A constant (compared with `==`).
    Const(DefId),
}

/// The operation of a `select` arm (spec §11.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectOp {
    /// `ch.recv`: the channel expression.
    Recv {
        /// The channel.
        channel: ExprId,
    },
    /// `ch.send(value)`.
    Send {
        /// The channel.
        channel: ExprId,
        /// The value sent.
        value: ExprId,
    },
    /// `Time.after(d)`: the operation expression evaluates to a `Timeout`.
    Timer,
}

/// The result of checking one body.
#[derive(Clone, Debug)]
pub struct TypedBody {
    /// The type of every expression.
    pub expr_tys: Vec<Ty>,
    /// The type of every pattern.
    pub pat_tys: Vec<Ty>,
    /// The type of every binding.
    pub binding_tys: Vec<Ty>,
    /// Meanings of expressions.
    pub exprs: HashMap<ExprId, ExprInfo>,
    /// Meanings of path patterns.
    pub pats: HashMap<PatId, PatInfo>,
    /// Implicit conversions.
    pub coercions: HashMap<ExprId, Coercion>,
    /// For each `x = e` that is an assignment: the binding it assigns.
    pub assignments: HashMap<BindingId, BindingId>,
    /// Or-pattern binding aliases.
    pub aliases: HashMap<BindingId, BindingId>,
    /// The operations of the arms of each `select`, in order.
    pub selects: HashMap<ExprId, Vec<SelectOp>>,
}

impl TypedBody {
    /// The type of `expr`.
    #[must_use]
    pub fn expr_ty(&self, expr: ExprId) -> Ty {
        self.expr_tys[expr.index()]
    }

    /// The canonical binding for `binding`.
    #[must_use]
    pub fn canonical(&self, binding: BindingId) -> BindingId {
        self.aliases.get(&binding).copied().unwrap_or(binding)
    }
}

/// The checked program.
#[derive(Debug)]
pub struct Checked {
    /// Types.
    pub interner: Interner,
    /// Signatures.
    pub items: Items,
    /// Checked bodies.
    pub bodies: HashMap<BodyId, TypedBody>,
    /// Errors and warnings.
    pub diagnostics: Vec<Diagnostic>,
}

/// Shared read-only context.
pub(crate) struct Ctx<'a> {
    pub hir: &'a Hir,
    pub def_map: &'a DefMap,
    pub resolution: &'a Resolution,
    pub items: &'a Items,
    pub interner: &'a Interner,
}

/// Checks the whole program.
#[must_use]
pub fn check(hir: &Hir, resolution: &Resolution) -> Checked {
    let interner = Interner::new();
    let mut diagnostics = Vec::new();
    let items = items::collect(hir, &resolution.def_map, &interner, &mut diagnostics);
    let cx = Ctx {
        hir,
        def_map: &resolution.def_map,
        resolution,
        items: &items,
        interner: &interner,
    };
    item_checks::check_items(&cx, &mut diagnostics);
    let mut bodies = HashMap::new();
    for (index, _) in hir.bodies.iter().enumerate() {
        let id = BodyId(u32::try_from(index).unwrap_or(u32::MAX));
        let typed = infer::infer_body(&cx, id, &mut diagnostics);
        bodies.insert(id, typed);
    }
    Checked {
        interner,
        items,
        bodies,
        diagnostics,
    }
}
