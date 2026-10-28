//! Types: representation, interning, substitution and unification (ADR 0019 §3).
//!
//! A [`Ty`] is a `Copy` index into an [`Interner`]; equal types have equal indices, so
//! comparing types is comparing integers. Inference variables are solved in an
//! [`InferTable`] with union-find.

use loom_hir::{DefId, FnKind};
use loom_resolve::TyParamId;
use std::cell::RefCell;
use std::collections::HashMap;

/// An interned type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ty(u32);

/// Integer types (spec §5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IntTy {
    /// `I8`.
    I8,
    /// `I16`.
    I16,
    /// `I32`.
    I32,
    /// `I64` (`Int`).
    I64,
    /// `I128`.
    I128,
    /// `ISize`.
    ISize,
    /// `U8`.
    U8,
    /// `U16`.
    U16,
    /// `U32`.
    U32,
    /// `U64`.
    U64,
    /// `U128`.
    U128,
    /// `USize`.
    USize,
}

impl IntTy {
    /// Every integer type.
    pub const ALL: [Self; 12] = [
        Self::I8,
        Self::I16,
        Self::I32,
        Self::I64,
        Self::I128,
        Self::ISize,
        Self::U8,
        Self::U16,
        Self::U32,
        Self::U64,
        Self::U128,
        Self::USize,
    ];

    /// The type's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::I8 => "I8",
            Self::I16 => "I16",
            Self::I32 => "I32",
            Self::I64 => "Int",
            Self::I128 => "I128",
            Self::ISize => "ISize",
            Self::U8 => "U8",
            Self::U16 => "U16",
            Self::U32 => "U32",
            Self::U64 => "U64",
            Self::U128 => "U128",
            Self::USize => "USize",
        }
    }

    /// The lang-item name of the type's core-library struct.
    #[must_use]
    pub fn lang(self) -> &'static str {
        match self {
            Self::I64 => "I64",
            other => other.name(),
        }
    }

    /// Whether the type is signed.
    #[must_use]
    pub fn is_signed(self) -> bool {
        matches!(
            self,
            Self::I8 | Self::I16 | Self::I32 | Self::I64 | Self::I128 | Self::ISize
        )
    }

    /// The width in bits (64 for the pointer-sized types on every supported target).
    #[must_use]
    pub fn bits(self) -> u32 {
        match self {
            Self::I8 | Self::U8 => 8,
            Self::I16 | Self::U16 => 16,
            Self::I32 | Self::U32 => 32,
            Self::I64 | Self::U64 | Self::ISize | Self::USize => 64,
            Self::I128 | Self::U128 => 128,
        }
    }

    /// The smallest value.
    #[must_use]
    pub fn min(self) -> i128 {
        if self.is_signed() {
            if self.bits() == 128 {
                i128::MIN
            } else {
                -(1i128 << (self.bits() - 1))
            }
        } else {
            0
        }
    }

    /// The largest value, as `u128`.
    #[must_use]
    pub fn max(self) -> u128 {
        match (self.is_signed(), self.bits()) {
            (true, bits) => (1u128 << (bits - 1)) - 1,
            (false, 128) => u128::MAX,
            (false, bits) => (1u128 << bits) - 1,
        }
    }
}

/// Float types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FloatTy {
    /// `F32`.
    F32,
    /// `F64` (`Float`).
    F64,
}

impl FloatTy {
    /// The type's name.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::F32 => "F32",
            Self::F64 => "Float",
        }
    }

    /// The lang-item name of the type's core-library struct.
    #[must_use]
    pub fn lang(self) -> &'static str {
        match self {
            Self::F32 => "F32",
            Self::F64 => "F64",
        }
    }
}

/// An inference variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InferVar(pub u32);

/// The kinds of inference variables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VarKind {
    /// Any type.
    General,
    /// An integer type (from an integer literal).
    Int,
    /// A float type (from a float literal).
    Float,
}

/// The shape of a type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TyKind {
    /// An integer type.
    Int(IntTy),
    /// A float type.
    Float(FloatTy),
    /// `Bool`.
    Bool,
    /// `Char`.
    Char,
    /// `String`.
    String,
    /// `Bytes`.
    Bytes,
    /// `Unit` / `()`.
    Unit,
    /// `Never`.
    Never,
    /// A tuple of two or more elements.
    Tuple(Vec<Ty>),
    /// A struct or enum with its generic arguments.
    Adt(DefId, Vec<Ty>),
    /// A generic parameter.
    Param(TyParamId),
    /// An associated type that could not be normalized yet: `<self_ty as Trait[args]>.Assoc`.
    Projection {
        /// The trait declaring the associated type.
        trait_: DefId,
        /// The trait's generic arguments (with the self type first).
        args: Vec<Ty>,
        /// The associated type's declaration.
        assoc: DefId,
    },
    /// `Fn(A) -> R`, `FnMut`, `FnOnce`.
    Callable {
        /// The callable kind.
        kind: FnKind,
        /// Parameter types.
        params: Vec<Ty>,
        /// The return type.
        ret: Ty,
    },
    /// `dyn Trait[args]` (args exclude the self type).
    Dyn(DefId, Vec<Ty>),
    /// An inference variable.
    Infer(InferVar, VarKind),
    /// An error has been reported; unifies with everything.
    Error,
}

/// The `Self` parameter of a trait is the trait's parameter with this index.
pub const SELF_INDEX: u32 = u32::MAX;

/// Interns types. Interning goes through `&self`, so the interner can be shared.
#[derive(Debug, Default)]
pub struct Interner {
    inner: RefCell<InternerInner>,
}

#[derive(Debug, Default)]
struct InternerInner {
    kinds: Vec<TyKind>,
    map: HashMap<TyKind, Ty>,
}

impl Interner {
    /// Creates an interner.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns `kind`.
    pub fn intern(&self, kind: TyKind) -> Ty {
        let mut inner = self.inner.borrow_mut();
        if let Some(&ty) = inner.map.get(&kind) {
            return ty;
        }
        let ty = Ty(u32::try_from(inner.kinds.len()).unwrap_or(u32::MAX));
        inner.kinds.push(kind.clone());
        inner.map.insert(kind, ty);
        ty
    }

    /// The shape of `ty`.
    #[must_use]
    pub fn kind(&self, ty: Ty) -> TyKind {
        self.inner.borrow().kinds[ty.0 as usize].clone()
    }

    /// `Unit`.
    pub fn unit(&self) -> Ty {
        self.intern(TyKind::Unit)
    }

    /// `Never`.
    pub fn never(&self) -> Ty {
        self.intern(TyKind::Never)
    }

    /// `Bool`.
    pub fn bool(&self) -> Ty {
        self.intern(TyKind::Bool)
    }

    /// `Int`.
    pub fn int(&self) -> Ty {
        self.intern(TyKind::Int(IntTy::I64))
    }

    /// `String`.
    pub fn string(&self) -> Ty {
        self.intern(TyKind::String)
    }

    /// The error type.
    pub fn error(&self) -> Ty {
        self.intern(TyKind::Error)
    }

    /// Whether `ty` is the error type.
    #[must_use]
    pub fn is_error(&self, ty: Ty) -> bool {
        matches!(self.kind(ty), TyKind::Error)
    }

    /// Applies `f` to every immediate component of `ty` and rebuilds it.
    pub fn map_children(&self, ty: Ty, f: &mut impl FnMut(Ty) -> Ty) -> Ty {
        let kind = self.kind(ty);
        let mapped = match kind {
            TyKind::Tuple(elements) => TyKind::Tuple(elements.into_iter().map(&mut *f).collect()),
            TyKind::Adt(def, args) => TyKind::Adt(def, args.into_iter().map(&mut *f).collect()),
            TyKind::Projection {
                trait_,
                args,
                assoc,
            } => TyKind::Projection {
                trait_,
                args: args.into_iter().map(&mut *f).collect(),
                assoc,
            },
            TyKind::Callable { kind, params, ret } => TyKind::Callable {
                kind,
                params: params.into_iter().map(&mut *f).collect(),
                ret: f(ret),
            },
            TyKind::Dyn(def, args) => TyKind::Dyn(def, args.into_iter().map(&mut *f).collect()),
            _ => return ty,
        };
        self.intern(mapped)
    }

    /// Replaces generic parameters according to `subst`.
    pub fn subst(&self, ty: Ty, subst: &Subst) -> Ty {
        if subst.is_empty() {
            return ty;
        }
        match self.kind(ty) {
            TyKind::Param(param) => subst.get(param).unwrap_or(ty),
            _ => self.map_children(ty, &mut |t| self.subst(t, subst)),
        }
    }

    /// Whether `ty` mentions an inference variable.
    #[must_use]
    pub fn has_infer(&self, ty: Ty) -> bool {
        self.any(ty, &|k| matches!(k, TyKind::Infer(..)))
    }

    /// Whether `ty` mentions a generic parameter.
    #[must_use]
    pub fn has_param(&self, ty: Ty) -> bool {
        self.any(ty, &|k| matches!(k, TyKind::Param(_)))
    }

    /// Whether any component of `ty` satisfies `pred`.
    #[must_use]
    pub fn any(&self, ty: Ty, pred: &dyn Fn(&TyKind) -> bool) -> bool {
        let kind = self.kind(ty);
        if pred(&kind) {
            return true;
        }
        match kind {
            TyKind::Tuple(elements) | TyKind::Adt(_, elements) | TyKind::Dyn(_, elements) => {
                elements.into_iter().any(|t| self.any(t, pred))
            }
            TyKind::Projection { args, .. } => args.into_iter().any(|t| self.any(t, pred)),
            TyKind::Callable { params, ret, .. } => {
                params.into_iter().any(|t| self.any(t, pred)) || self.any(ret, pred)
            }
            _ => false,
        }
    }
}

/// A substitution of generic parameters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subst {
    map: Vec<(TyParamId, Ty)>,
}

impl Subst {
    /// The empty substitution.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether nothing is substituted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Maps `param` to `ty` (replacing an earlier mapping).
    pub fn insert(&mut self, param: TyParamId, ty: Ty) {
        if let Some(entry) = self.map.iter_mut().find(|(p, _)| *p == param) {
            entry.1 = ty;
        } else {
            self.map.push((param, ty));
        }
    }

    /// The type substituted for `param`.
    #[must_use]
    pub fn get(&self, param: TyParamId) -> Option<Ty> {
        self.map.iter().find(|(p, _)| *p == param).map(|&(_, t)| t)
    }

    /// Every mapping.
    pub fn iter(&self) -> impl Iterator<Item = (TyParamId, Ty)> + '_ {
        self.map.iter().copied()
    }

    /// Applies `f` to every substituted type.
    #[must_use]
    pub fn map(&self, mut f: impl FnMut(Ty) -> Ty) -> Self {
        Self {
            map: self.map.iter().map(|&(p, t)| (p, f(t))).collect(),
        }
    }

    /// Adds every mapping of `other` (which wins on conflicts).
    pub fn extend(&mut self, other: &Self) {
        for (p, t) in other.iter() {
            self.insert(p, t);
        }
    }
}

/// Two types could not be unified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch;

/// Solves inference variables by unification.
#[derive(Debug, Default)]
pub struct InferTable {
    /// For each variable: its kind and its binding, if solved.
    vars: Vec<(VarKind, Option<Ty>)>,
}

impl InferTable {
    /// Creates an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh variable of `kind`.
    pub fn new_var(&mut self, interner: &Interner, kind: VarKind) -> Ty {
        let var = InferVar(u32::try_from(self.vars.len()).unwrap_or(u32::MAX));
        self.vars.push((kind, None));
        interner.intern(TyKind::Infer(var, kind))
    }

    /// Follows solved variables at the top of `ty`.
    #[must_use]
    pub fn shallow(&self, interner: &Interner, mut ty: Ty) -> Ty {
        loop {
            match interner.kind(ty) {
                TyKind::Infer(var, _) => match self.vars[var.0 as usize].1 {
                    Some(bound) => ty = bound,
                    None => return ty,
                },
                _ => return ty,
            }
        }
    }

    /// Replaces every solved variable in `ty`.
    pub fn resolve(&self, interner: &Interner, ty: Ty) -> Ty {
        let ty = self.shallow(interner, ty);
        if !interner.has_infer(ty) {
            return ty;
        }
        interner.map_children(ty, &mut |t| self.resolve(interner, t))
    }

    /// Every unsolved variable, with its kind.
    pub fn unsolved(&self) -> impl Iterator<Item = (InferVar, VarKind)> + '_ {
        self.vars
            .iter()
            .enumerate()
            .filter(|(_, (_, bound))| bound.is_none())
            .map(|(i, &(kind, _))| (InferVar(u32::try_from(i).unwrap_or(u32::MAX)), kind))
    }

    /// Solves `var` to `ty` without checks (used for defaulting).
    pub fn bind(&mut self, var: InferVar, ty: Ty) {
        self.vars[var.0 as usize].1 = Some(ty);
    }

    /// Makes `a` and `b` equal. On failure, nothing is guaranteed about partially
    /// unified components; callers report the error and continue.
    ///
    /// # Errors
    ///
    /// If the types cannot be made equal.
    pub fn unify(&mut self, interner: &Interner, a: Ty, b: Ty) -> Result<(), Mismatch> {
        let a = self.shallow(interner, a);
        let b = self.shallow(interner, b);
        if a == b {
            return Ok(());
        }
        match (interner.kind(a), interner.kind(b)) {
            (TyKind::Error, _) | (_, TyKind::Error) => Ok(()),
            (TyKind::Infer(va, ka), TyKind::Infer(vb, kb)) => {
                let kind = match (ka, kb) {
                    (VarKind::General, k) | (k, VarKind::General) => k,
                    (x, y) if x == y => x,
                    _ => return Err(Mismatch),
                };
                self.vars[va.0 as usize].0 = kind;
                self.vars[vb.0 as usize].0 = kind;
                self.vars[vb.0 as usize].1 = Some(a);
                Ok(())
            }
            (TyKind::Infer(var, kind), other) => self.bind_checked(interner, var, kind, &other, b),
            (other, TyKind::Infer(var, kind)) => self.bind_checked(interner, var, kind, &other, a),
            (TyKind::Tuple(x), TyKind::Tuple(y)) => self.unify_all(interner, &x, &y),
            (TyKind::Adt(dx, x), TyKind::Adt(dy, y)) if dx == dy => {
                self.unify_all(interner, &x, &y)
            }
            (TyKind::Dyn(dx, x), TyKind::Dyn(dy, y)) if dx == dy => {
                self.unify_all(interner, &x, &y)
            }
            (
                TyKind::Callable {
                    kind: kx,
                    params: px,
                    ret: rx,
                },
                TyKind::Callable {
                    kind: ky,
                    params: py,
                    ret: ry,
                },
            ) if kx == ky => {
                self.unify_all(interner, &px, &py)?;
                self.unify(interner, rx, ry)
            }
            (
                TyKind::Projection {
                    trait_: tx,
                    args: ax,
                    assoc: sx,
                },
                TyKind::Projection {
                    trait_: ty,
                    args: ay,
                    assoc: sy,
                },
            ) if tx == ty && sx == sy => self.unify_all(interner, &ax, &ay),
            _ => Err(Mismatch),
        }
    }

    fn unify_all(&mut self, interner: &Interner, x: &[Ty], y: &[Ty]) -> Result<(), Mismatch> {
        if x.len() != y.len() {
            return Err(Mismatch);
        }
        for (&a, &b) in x.iter().zip(y) {
            self.unify(interner, a, b)?;
        }
        Ok(())
    }

    fn bind_checked(
        &mut self,
        interner: &Interner,
        var: InferVar,
        kind: VarKind,
        other: &TyKind,
        ty: Ty,
    ) -> Result<(), Mismatch> {
        let fits = match kind {
            VarKind::General => true,
            VarKind::Int => matches!(other, TyKind::Int(_)),
            VarKind::Float => matches!(other, TyKind::Float(_)),
        };
        if !fits || self.occurs(interner, var, ty) {
            return Err(Mismatch);
        }
        self.vars[var.0 as usize].1 = Some(ty);
        Ok(())
    }

    fn occurs(&self, interner: &Interner, var: InferVar, ty: Ty) -> bool {
        let ty = self.shallow(interner, ty);
        match interner.kind(ty) {
            TyKind::Infer(v, _) => v == var,
            TyKind::Tuple(elements) | TyKind::Adt(_, elements) | TyKind::Dyn(_, elements) => {
                elements.into_iter().any(|t| self.occurs(interner, var, t))
            }
            TyKind::Projection { args, .. } => {
                args.into_iter().any(|t| self.occurs(interner, var, t))
            }
            TyKind::Callable { params, ret, .. } => {
                params.into_iter().any(|t| self.occurs(interner, var, t))
                    || self.occurs(interner, var, ret)
            }
            _ => false,
        }
    }

    /// Takes a snapshot that [`InferTable::rollback`] can restore.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(VarKind, Option<Ty>)> {
        self.vars.clone()
    }

    /// Restores a snapshot.
    pub fn rollback(&mut self, snapshot: Vec<(VarKind, Option<Ty>)>) {
        self.vars = snapshot;
    }
}

#[cfg(test)]
mod tests {
    use super::{InferTable, IntTy, Interner, TyKind, VarKind};

    #[test]
    fn unifies_and_resolves() {
        let interner = Interner::new();
        let mut table = InferTable::new();
        let v = table.new_var(&interner, VarKind::General);
        let int = interner.int();
        let tuple = interner.intern(TyKind::Tuple(vec![v, int]));
        let target = interner.intern(TyKind::Tuple(vec![interner.bool(), int]));
        assert!(table.unify(&interner, tuple, target).is_ok());
        assert_eq!(table.resolve(&interner, tuple), target);
    }

    #[test]
    fn integer_variables_only_take_integers() {
        let interner = Interner::new();
        let mut table = InferTable::new();
        let v = table.new_var(&interner, VarKind::Int);
        assert!(table.unify(&interner, v, interner.bool()).is_err());
        let u8 = interner.intern(TyKind::Int(IntTy::U8));
        assert!(table.unify(&interner, v, u8).is_ok());
        assert_eq!(table.resolve(&interner, v), u8);
    }

    #[test]
    fn occurs_check() {
        let interner = Interner::new();
        let mut table = InferTable::new();
        let v = table.new_var(&interner, VarKind::General);
        let tuple = interner.intern(TyKind::Tuple(vec![v, v]));
        assert!(table.unify(&interner, v, tuple).is_err());
    }

    #[test]
    fn integer_ranges() {
        assert_eq!(IntTy::I8.min(), -128);
        assert_eq!(IntTy::I8.max(), 127);
        assert_eq!(IntTy::U64.max(), u128::from(u64::MAX));
        assert_eq!(IntTy::I128.min(), i128::MIN);
    }
}
