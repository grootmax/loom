//! The mid-level IR (ADR 0020 §2): a control-flow graph per function and per closure,
//! built from HIR and the checker's `TypedBody`, and the analyses that run on it: the
//! ownership and borrow checker ([`borrowck`]) and drop elaboration.
//!
//! The shape follows Rust's MIR. Borrow-mode and `mut` parameters (and receivers) are
//! explicit: their locals have type `ref T` / `mut ref T`, and arguments for them are
//! `Rvalue::Ref` of a place, so every borrow the program makes is visible here.

pub mod borrowck;
mod build;
mod pretty;

pub use build::build_program;
pub use pretty::dump_body;

use loom_hir::{BinaryOp, BindingId, BodyId, DefId, ExprId, UnaryOp};
use loom_span::Span;
use loom_types::{FloatTy, IntTy, Subst, Ty};
use std::rc::Rc;

macro_rules! idx {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
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

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!($prefix, "{}"), self.0)
            }
        }
    };
}

idx!(
    /// A local variable. `_0` is the return place; arguments follow.
    Local, "_"
);
idx!(
    /// A basic block.
    BlockId, "bb"
);

/// Which source construct a MIR body comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MirSource {
    /// A function, method or constant body.
    Item(BodyId),
    /// A closure expression inside a body.
    Closure(BodyId, ExprId),
    /// The default value of a function's parameter (by position): a function of the
    /// parameters before it, evaluated at each call that omits the argument.
    Default(BodyId, u32),
}

/// What a local stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalKind {
    /// The return place.
    Return,
    /// The receiver.
    SelfArg,
    /// A parameter, by position (excluding the receiver).
    Arg(u32),
    /// The environment of a closure.
    ClosureEnv,
    /// A user binding.
    User(BindingId),
    /// A compiler temporary.
    Temp,
}

/// A local's declaration.
#[derive(Clone, Debug)]
pub struct LocalDecl {
    /// Its type (generic: may mention type parameters).
    pub ty: Ty,
    /// What it stands for.
    pub kind: LocalKind,
    /// Whether it may be assigned after initialization (`var`, `mut` parameter targets).
    pub mutable: bool,
    /// Where it was declared.
    pub span: Span,
}

/// One step of a place projection.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Proj {
    /// Follow a `ref` or `mut ref`.
    Deref,
    /// A field of a struct, tuple or (after a downcast) variant.
    Field(u32),
    /// View an enum value as one of its variants.
    Downcast(DefId),
    /// An array element; the index is a local of type `Int`. Out of bounds panics.
    Index(Local),
}

/// A memory location: a local and a path into it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    /// The root local.
    pub local: Local,
    /// The projections, outermost first.
    pub proj: Vec<Proj>,
}

impl Place {
    /// The whole of `local`.
    #[must_use]
    pub fn local(local: Local) -> Self {
        Self {
            local,
            proj: Vec::new(),
        }
    }

    /// This place followed by `step`.
    #[must_use]
    pub fn project(&self, step: Proj) -> Self {
        let mut proj = self.proj.clone();
        proj.push(step);
        Self {
            local: self.local,
            proj,
        }
    }

    /// Whether the place goes through a `Deref` (and so is behind a borrow).
    #[must_use]
    pub fn is_behind_ref(&self) -> bool {
        self.proj.contains(&Proj::Deref)
    }

    /// Whether `self` is `other` or contains it, or the reverse.
    #[must_use]
    pub fn overlaps(&self, other: &Self) -> bool {
        if self.local != other.local {
            return false;
        }
        self.proj
            .iter()
            .zip(&other.proj)
            .all(|(a, b)| match (a, b) {
                (Proj::Field(x), Proj::Field(y)) => x == y,
                (Proj::Downcast(x), Proj::Downcast(y)) => x == y,
                _ => true,
            })
    }

    /// Whether `self` is a prefix of (contains) `other`.
    #[must_use]
    pub fn is_prefix_of(&self, other: &Self) -> bool {
        self.local == other.local
            && self.proj.len() <= other.proj.len()
            && self.proj.iter().zip(&other.proj).all(|(a, b)| a == b)
    }
}

/// A constant value.
#[derive(Clone, Debug, PartialEq)]
pub enum Constant {
    /// An integer of the given type (`U128` as its bit pattern).
    Int(i128, IntTy),
    /// A float.
    Float(f64, FloatTy),
    /// A boolean.
    Bool(bool),
    /// A character.
    Char(char),
    /// A string literal.
    Str(Rc<str>),
    /// A byte string literal.
    Bytes(Rc<[u8]>),
    /// `()`.
    Unit,
    /// A function (or variant constructor) used as a value.
    Fn {
        /// The function.
        def: DefId,
        /// Its generic arguments.
        subst: Subst,
    },
    /// A constant item's value.
    Item(DefId),
}

/// An operand of an rvalue or call.
#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    /// Read a place of a Copy type (or a borrow).
    Copy(Place),
    /// Move a place: it may not be used again until reassigned.
    Move(Place),
    /// A constant.
    Const(Constant, Ty),
}

/// What an aggregate builds.
#[derive(Clone, Debug, PartialEq)]
pub enum AggregateKind {
    /// A tuple.
    Tuple,
    /// A struct, fields in declaration order.
    Struct(DefId, Subst),
    /// An enum variant, payload fields in order.
    Variant(DefId, Subst),
    /// An array literal.
    Array(Ty),
    /// A map literal: operands alternate key, value.
    Map(Ty, Ty),
    /// A closure value: its captures, in the order of [`MirBody::captures`] of the
    /// closure's body.
    Closure(MirSource),
}

/// The right-hand side of an assignment.
#[derive(Clone, Debug, PartialEq)]
pub enum Rvalue {
    /// The operand's value.
    Use(Operand),
    /// A borrow of a place.
    Ref {
        /// `mut ref`.
        mutable: bool,
        /// The borrowed place.
        place: Place,
    },
    /// A binary operation (overflow and division by zero panic, spec §7.5).
    Binary(BinaryOp, Operand, Operand),
    /// A unary operation (negation overflow panics).
    Unary(UnaryOp, Operand),
    /// A struct, variant, tuple, array, map or closure.
    Aggregate(AggregateKind, Vec<Operand>),
    /// The variant index of an enum place.
    Discriminant(Place),
    /// A value converted to `dyn Trait` (boxed).
    Unsize(Operand, Ty),
}

/// A statement.
#[derive(Clone, Debug, PartialEq)]
pub enum StatementKind {
    /// `place = rvalue`.
    Assign(Place, Rvalue),
    /// The local's storage begins (its scope starts).
    StorageLive(Local),
    /// The local's storage ends (its scope ends).
    StorageDead(Local),
}

/// A statement with its source position.
#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    /// What it does.
    pub kind: StatementKind,
    /// Where it comes from.
    pub span: Span,
}

/// What a call invokes.
#[derive(Clone, Debug, PartialEq)]
pub enum Callee {
    /// A known function (including intrinsics), with generic arguments.
    Fn {
        /// The function.
        def: DefId,
        /// Its generic arguments.
        subst: Subst,
    },
    /// A trait method, resolved for the concrete `self_ty` when monomorphized.
    Trait {
        /// The trait method's declaration.
        method: DefId,
        /// The implementing type.
        self_ty: Ty,
        /// The trait's and the method's generic arguments.
        subst: Subst,
    },
    /// A value of a callable type.
    Value(Operand),
    /// The default of parameter `index` of `def`, given the arguments before it.
    Default {
        /// The function whose parameter it is.
        def: DefId,
        /// The parameter's position.
        index: u32,
        /// The function's generic arguments.
        subst: Subst,
    },
    /// A built-in operation of the language that is not a core function (string
    /// building for interpolation, map indexing).
    Builtin(&'static str),
}

/// How a block ends.
#[derive(Clone, Debug, PartialEq)]
pub enum TerminatorKind {
    /// Jump.
    Goto(BlockId),
    /// Branch on an integer (a `Bool` is 0 or 1; a discriminant is a variant index).
    SwitchInt {
        /// The value tested.
        discr: Operand,
        /// `(value, target)` pairs.
        targets: Vec<(u128, BlockId)>,
        /// The target for every other value.
        otherwise: BlockId,
    },
    /// A call. The receiver, if any, is the first argument.
    Call {
        /// What is called.
        callee: Callee,
        /// The arguments, receiver first.
        args: Vec<Operand>,
        /// Where the result goes.
        dest: Place,
        /// Where execution continues (`None` if the callee never returns).
        target: Option<BlockId>,
    },
    /// Drop the value at `place` (if it is still initialized), then continue.
    Drop {
        /// The place dropped.
        place: Place,
        /// Where execution continues.
        target: BlockId,
    },
    /// Return `_0`.
    Return,
    /// Panic with a message.
    Panic(Rc<str>),
    /// Unreachable code (after a call that never returns).
    Unreachable,
}

/// A terminator with its source position.
#[derive(Clone, Debug, PartialEq)]
pub struct Terminator {
    /// What it does.
    pub kind: TerminatorKind,
    /// Where it comes from.
    pub span: Span,
}

/// A basic block.
#[derive(Clone, Debug, PartialEq)]
pub struct BasicBlock {
    /// Its statements.
    pub statements: Vec<Statement>,
    /// Its terminator (`None` only while building).
    pub terminator: Option<Terminator>,
}

impl BasicBlock {
    /// The successors of this block.
    #[must_use]
    pub fn successors(&self) -> Vec<BlockId> {
        match self.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::Goto(b)) => vec![*b],
            Some(TerminatorKind::SwitchInt {
                targets, otherwise, ..
            }) => {
                let mut out: Vec<BlockId> = targets.iter().map(|&(_, b)| b).collect();
                out.push(*otherwise);
                out
            }
            Some(TerminatorKind::Call { target, .. }) => target.iter().copied().collect(),
            Some(TerminatorKind::Drop { target, .. }) => vec![*target],
            _ => Vec::new(),
        }
    }
}

/// A captured binding of a closure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capture {
    /// The binding of the enclosing body.
    pub binding: BindingId,
    /// `None` for a capture by move (or copy); `Some(mutable)` for a capture by reference.
    pub by_ref: Option<bool>,
    /// The captured type (the binding's type).
    pub ty: Ty,
}

/// One function or closure in MIR form.
#[derive(Clone, Debug)]
pub struct MirBody {
    /// Where it comes from.
    pub source: MirSource,
    /// The item that owns it (the function, or the function containing the closure).
    pub owner: DefId,
    /// Locals: `_0` is the return place, then the receiver and parameters.
    pub locals: Vec<LocalDecl>,
    /// The number of argument locals (receiver, closure environment and parameters).
    pub arg_count: usize,
    /// Basic blocks; `bb0` is the entry.
    pub blocks: Vec<BasicBlock>,
    /// For a closure: what it captures, in environment order.
    pub captures: Vec<Capture>,
    /// For a closure: whether it escapes (captures by move, spec §8.4).
    pub escaping: bool,
    /// Where the body is.
    pub span: Span,
}

impl MirBody {
    /// The declaration of `local`.
    #[must_use]
    pub fn local(&self, local: Local) -> &LocalDecl {
        &self.locals[local.index()]
    }

    /// Every block id.
    pub fn block_ids(&self) -> impl Iterator<Item = BlockId> + use<> {
        (0..u32::try_from(self.blocks.len()).unwrap_or(u32::MAX)).map(BlockId)
    }
}

/// Every MIR body of a program.
#[derive(Debug, Default)]
pub struct Mir {
    /// The bodies.
    pub bodies: Vec<MirBody>,
    /// The body for each source.
    pub by_source: std::collections::HashMap<MirSource, usize>,
}

impl Mir {
    /// The body built from `source`.
    #[must_use]
    pub fn body(&self, source: MirSource) -> Option<&MirBody> {
        self.by_source.get(&source).map(|&i| &self.bodies[i])
    }
}

/// The type of `place` in `body` (generic).
#[must_use]
pub fn place_ty(
    hir: &loom_hir::Hir,
    items: &loom_check::Items,
    interner: &loom_types::Interner,
    body: &MirBody,
    place: &Place,
) -> Ty {
    use loom_types::TyKind;
    let mut ty = body.local(place.local).ty;
    let mut variant: Option<DefId> = None;
    for step in &place.proj {
        match step {
            Proj::Deref => {
                if let TyKind::Ref { inner, .. } = interner.kind(ty) {
                    ty = inner;
                }
            }
            Proj::Downcast(v) => variant = Some(*v),
            Proj::Index(_) => {
                if let TyKind::Adt(_, args) = interner.kind(ty) {
                    ty = args.first().copied().unwrap_or_else(|| interner.error());
                }
            }
            Proj::Field(i) => {
                let i = *i as usize;
                ty = match (interner.kind(ty), variant.take()) {
                    (TyKind::Tuple(elements), _) => {
                        elements.get(i).copied().unwrap_or_else(|| interner.error())
                    }
                    (TyKind::Adt(def, args), Some(v)) => {
                        let subst = items.adt_subst(def, &args);
                        match items.variants.get(&v) {
                            Some(loom_check::VariantTy::Positional(types)) => types
                                .get(i)
                                .map_or_else(|| interner.error(), |&t| interner.subst(t, &subst)),
                            Some(loom_check::VariantTy::Named(fields)) => fields
                                .get(i)
                                .map_or_else(|| interner.error(), |f| interner.subst(f.ty, &subst)),
                            _ => interner.error(),
                        }
                    }
                    (TyKind::Adt(def, args), None) => {
                        let subst = items.adt_subst(def, &args);
                        items
                            .fields
                            .get(&def)
                            .and_then(|fs| fs.get(i))
                            .map_or_else(|| interner.error(), |f| interner.subst(f.ty, &subst))
                    }
                    (TyKind::Callable { .. }, _) => {
                        // A closure environment: the capture's type.
                        let _ = hir;
                        interner.error()
                    }
                    _ => interner.error(),
                };
            }
        }
    }
    ty
}
