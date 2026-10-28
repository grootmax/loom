//! Trait solving: selecting implementations, normalizing associated types and
//! elaborating bounds (spec §6.6). Used by the checker and, with concrete types, by the
//! interpreter.

use crate::items::{ImplId, Items, Predicate, TraitRef};
use loom_hir::{DefId, Hir};
use loom_types::{InferTable, Interner, Subst, Ty, TyKind, VarKind};

/// How a trait obligation is satisfied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection {
    /// By an implementation, with its parameters instantiated.
    Impl {
        /// The implementation.
        impl_id: ImplId,
        /// Its parameters.
        subst: Subst,
    },
    /// By a bound in scope (the type is a generic parameter or a rigid projection).
    Param,
    /// By a trait object.
    Dyn,
    /// By the language (primitives, tuples).
    Builtin,
}

/// Why an obligation is not (yet) satisfied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectError {
    /// No implementation applies.
    NoImpl,
    /// The type is not known well enough yet.
    Ambiguous,
}

/// The predicates assumed to hold, elaborated with supertraits and associated type bounds.
#[derive(Clone, Debug, Default)]
pub struct Env {
    /// The predicates.
    pub predicates: Vec<Predicate>,
}

/// Solves trait obligations.
pub struct Solver<'a> {
    /// The program.
    pub hir: &'a Hir,
    /// Signatures.
    pub items: &'a Items,
    /// Types.
    pub interner: &'a Interner,
    /// Inference variables.
    pub table: &'a mut InferTable,
}

const MAX_DEPTH: u32 = 24;

impl Solver<'_> {
    fn lang_of(&self, trait_: DefId) -> &str {
        self.hir.def(trait_).lang().unwrap_or("")
    }

    /// Builds the environment for `predicates`.
    #[must_use]
    pub fn elaborate(&self, predicates: &[Predicate]) -> Env {
        let mut out: Vec<Predicate> = Vec::new();
        let mut stack: Vec<Predicate> = predicates.to_vec();
        while let Some(pred) = stack.pop() {
            if out.contains(&pred) || out.len() > 256 {
                continue;
            }
            let Some(info) = self.items.traits.get(&pred.trait_ref.trait_) else {
                out.push(pred);
                continue;
            };
            let subst = self.trait_subst(&pred);
            for sup in &info.supertraits {
                stack.push(Predicate {
                    ty: pred.ty,
                    trait_ref: TraitRef {
                        trait_: sup.trait_,
                        args: sup
                            .args
                            .iter()
                            .map(|&a| self.interner.subst(a, &subst))
                            .collect(),
                    },
                });
            }
            for (&assoc, bounds) in &info.assoc_bounds {
                let mut args = vec![pred.ty];
                args.extend(pred.trait_ref.args.iter().copied());
                let projection = self.interner.intern(TyKind::Projection {
                    trait_: pred.trait_ref.trait_,
                    args,
                    assoc,
                });
                for bound in bounds {
                    stack.push(Predicate {
                        ty: projection,
                        trait_ref: TraitRef {
                            trait_: bound.trait_,
                            args: bound
                                .args
                                .iter()
                                .map(|&a| self.interner.subst(a, &subst))
                                .collect(),
                        },
                    });
                }
            }
            out.push(pred);
        }
        Env { predicates: out }
    }

    /// The substitution of a trait's `Self` and parameters for `pred`.
    #[must_use]
    pub fn trait_subst(&self, pred: &Predicate) -> Subst {
        let trait_ = pred.trait_ref.trait_;
        let mut subst = Subst::new();
        subst.insert(Items::self_param(trait_), pred.ty);
        for (&param, &arg) in self
            .items
            .generics_of(trait_)
            .own
            .iter()
            .zip(&pred.trait_ref.args)
        {
            subst.insert(param, arg);
        }
        subst
    }

    /// Whether `ty: trait_ref` holds (or may hold once inference finishes).
    pub fn may_hold(&mut self, ty: Ty, trait_ref: &TraitRef, env: &Env) -> bool {
        !matches!(self.select(ty, trait_ref, env), Err(SelectError::NoImpl))
    }

    /// Selects how `ty: trait_ref` is satisfied. On success, inference variables in `ty`
    /// and the trait's arguments may have been solved.
    ///
    /// # Errors
    ///
    /// [`SelectError::NoImpl`] or [`SelectError::Ambiguous`].
    pub fn select(
        &mut self,
        ty: Ty,
        trait_ref: &TraitRef,
        env: &Env,
    ) -> Result<Selection, SelectError> {
        self.select_at(ty, trait_ref, env, 0)
    }

    #[allow(clippy::too_many_lines, reason = "one case per kind of type")]
    fn select_at(
        &mut self,
        ty: Ty,
        trait_ref: &TraitRef,
        env: &Env,
        depth: u32,
    ) -> Result<Selection, SelectError> {
        if depth > MAX_DEPTH {
            return Err(SelectError::Ambiguous);
        }
        let ty = self.table.shallow(self.interner, ty);
        let ty = self.normalize_shallow(ty, env);
        let kind = self.interner.kind(ty);
        let lang = self.lang_of(trait_ref.trait_).to_owned();
        match &kind {
            TyKind::Error | TyKind::Never => return Ok(Selection::Builtin),
            TyKind::Infer(_, VarKind::General) => return Err(SelectError::Ambiguous),
            // Integer and float variables implement the structural traits; other traits
            // are decided by the impls that can apply to some integer (or float) type.
            TyKind::Infer(_, VarKind::Int)
                if matches!(
                    lang.as_str(),
                    "Eq" | "Ord" | "Clone" | "Copy" | "Hash" | "Default"
                ) =>
            {
                return Ok(Selection::Builtin);
            }
            TyKind::Infer(_, VarKind::Float)
                if matches!(lang.as_str(), "Eq" | "Ord" | "Clone" | "Copy" | "Default") =>
            {
                return Ok(Selection::Builtin);
            }
            TyKind::Param(_) | TyKind::Projection { .. } => {
                for pred in &env.predicates {
                    if pred.trait_ref.trait_ != trait_ref.trait_ {
                        continue;
                    }
                    let snapshot = self.table.snapshot();
                    if self.table.unify(self.interner, pred.ty, ty).is_ok()
                        && self.unify_args(&pred.trait_ref.args, &trait_ref.args)
                    {
                        return Ok(Selection::Param);
                    }
                    self.table.rollback(snapshot);
                }
                // A projection of a concrete type may still have impls; a parameter only
                // through blanket impls.
            }
            // A borrow is Copy and Clone; every other trait is the borrowed type's
            // (spec §8.3 rule 2).
            TyKind::Ref { inner, .. } => {
                return if matches!(lang.as_str(), "Copy" | "Clone") {
                    Ok(Selection::Builtin)
                } else {
                    self.select_at(*inner, trait_ref, env, depth + 1)
                };
            }
            TyKind::Dyn(dyn_trait, args) if self.trait_implies(*dyn_trait, args, ty, trait_ref) => {
                return Ok(Selection::Dyn);
            }
            _ => {}
        }
        if let Some(result) = self.builtin(&kind, &lang, env, depth) {
            return result;
        }
        let Some(candidates) = self.items.impls_by_trait.get(&trait_ref.trait_) else {
            return Err(SelectError::NoImpl);
        };
        let mut found: Vec<ImplId> = Vec::new();
        let mut ambiguous = false;
        for &impl_id in candidates {
            let snapshot = self.table.snapshot();
            match self.match_impl(impl_id, ty, trait_ref, env, depth) {
                Ok(_) => found.push(impl_id),
                Err(SelectError::Ambiguous) => ambiguous = true,
                Err(SelectError::NoImpl) => {}
            }
            self.table.rollback(snapshot);
        }
        match (found.as_slice(), ambiguous) {
            ([only], false) => {
                let subst = self.match_impl(*only, ty, trait_ref, env, depth)?;
                Ok(Selection::Impl {
                    impl_id: *only,
                    subst,
                })
            }
            ([], false) => Err(SelectError::NoImpl),
            _ => Err(SelectError::Ambiguous),
        }
    }

    fn unify_args(&mut self, a: &[Ty], b: &[Ty]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(&x, &y)| self.table.unify(self.interner, x, y).is_ok())
    }

    /// Whether a `dyn dyn_trait[args]` value implements `trait_ref` (through supertraits).
    fn trait_implies(
        &mut self,
        dyn_trait: DefId,
        args: &[Ty],
        ty: Ty,
        trait_ref: &TraitRef,
    ) -> bool {
        let start = Predicate {
            ty,
            trait_ref: TraitRef {
                trait_: dyn_trait,
                args: args.to_vec(),
            },
        };
        let env = self.elaborate(&[start]);
        env.predicates.iter().any(|p| {
            p.trait_ref.trait_ == trait_ref.trait_ && p.trait_ref.args.len() == trait_ref.args.len()
        })
    }

    /// Instantiates impl `impl_id` and matches it against `ty: trait_ref`.
    fn match_impl(
        &mut self,
        impl_id: ImplId,
        ty: Ty,
        trait_ref: &TraitRef,
        env: &Env,
        depth: u32,
    ) -> Result<Subst, SelectError> {
        let info = &self.items.impls[impl_id];
        let mut subst = Subst::new();
        for &param in &info.params {
            let var = self.table.new_var(self.interner, VarKind::General);
            subst.insert(param, var);
        }
        let impl_self = self.interner.subst(info.self_ty, &subst);
        if self.table.unify(self.interner, impl_self, ty).is_err() {
            return Err(SelectError::NoImpl);
        }
        let impl_args: Vec<Ty> = info
            .trait_ref
            .args
            .iter()
            .map(|&a| self.interner.subst(a, &subst))
            .collect();
        if !self.unify_args(&impl_args, &trait_ref.args) {
            return Err(SelectError::NoImpl);
        }
        let predicates: Vec<Predicate> = info
            .predicates
            .iter()
            .map(|p| Predicate {
                ty: self.interner.subst(p.ty, &subst),
                trait_ref: TraitRef {
                    trait_: p.trait_ref.trait_,
                    args: p
                        .trait_ref
                        .args
                        .iter()
                        .map(|&a| self.interner.subst(a, &subst))
                        .collect(),
                },
            })
            .collect();
        let mut ambiguous = false;
        for pred in predicates {
            match self.select_at(pred.ty, &pred.trait_ref, env, depth + 1) {
                Ok(_) => {}
                Err(SelectError::NoImpl) => return Err(SelectError::NoImpl),
                Err(SelectError::Ambiguous) => ambiguous = true,
            }
        }
        if ambiguous {
            return Err(SelectError::Ambiguous);
        }
        Ok(subst.map(|t| self.table.resolve(self.interner, t)))
    }

    /// Implementations provided by the language for primitive and tuple types.
    #[allow(
        clippy::match_same_arms,
        reason = "arms listed per case for readability"
    )]
    fn builtin(
        &mut self,
        kind: &TyKind,
        lang: &str,
        env: &Env,
        depth: u32,
    ) -> Option<Result<Selection, SelectError>> {
        let structural = matches!(lang, "Eq" | "Ord" | "Clone" | "Copy" | "Hash" | "Default");
        if !structural {
            return None;
        }
        let holds = match kind {
            TyKind::Int(_) | TyKind::Unit => true,
            TyKind::Float(_) => lang != "Hash",
            TyKind::Bool => true,
            TyKind::Char => lang != "Default",
            TyKind::String | TyKind::Bytes => {
                lang != "Copy" && !(lang == "Clone" && matches!(kind, TyKind::String))
            }
            TyKind::Tuple(elements) => {
                let trait_ = self.items.lang.get(lang)?;
                let trait_ref = TraitRef {
                    trait_,
                    args: Vec::new(),
                };
                let mut ambiguous = false;
                for &element in elements {
                    match self.select_at(element, &trait_ref, env, depth + 1) {
                        Ok(_) => {}
                        Err(SelectError::NoImpl) => return Some(Err(SelectError::NoImpl)),
                        Err(SelectError::Ambiguous) => ambiguous = true,
                    }
                }
                if ambiguous {
                    return Some(Err(SelectError::Ambiguous));
                }
                true
            }
            TyKind::Callable { .. } => lang == "Clone",
            _ => return None,
        };
        if holds {
            Some(Ok(Selection::Builtin))
        } else {
            None
        }
    }

    /// Normalizes the projection at the top of `ty`, if it can be.
    pub fn normalize_shallow(&mut self, ty: Ty, env: &Env) -> Ty {
        let TyKind::Projection {
            trait_,
            args,
            assoc,
        } = self.interner.kind(ty)
        else {
            return ty;
        };
        let Some((&self_ty, rest)) = args.split_first() else {
            return ty;
        };
        let self_ty = self.table.shallow(self.interner, self_ty);
        let self_ty = self.normalize_shallow(self_ty, env);
        let trait_ref = TraitRef {
            trait_,
            args: rest.to_vec(),
        };
        match self.interner.kind(self_ty) {
            TyKind::Infer(..)
            | TyKind::Param(_)
            | TyKind::Projection { .. }
            | TyKind::Dyn(..)
            | TyKind::Error => {
                let mut new_args = vec![self_ty];
                new_args.extend(rest.iter().copied());
                return self.interner.intern(TyKind::Projection {
                    trait_,
                    args: new_args,
                    assoc,
                });
            }
            _ => {}
        }
        match self.select(self_ty, &trait_ref, env) {
            Ok(Selection::Impl { impl_id, subst }) => {
                match self.items.impls[impl_id].assoc_types.get(&assoc) {
                    Some(&value) => {
                        let value = self.interner.subst(value, &subst);
                        self.normalize(value, env)
                    }
                    None => self.interner.error(),
                }
            }
            _ => ty,
        }
    }

    /// Normalizes every projection in `ty` that can be.
    pub fn normalize(&mut self, ty: Ty, env: &Env) -> Ty {
        let ty = self.table.resolve(self.interner, ty);
        if !self
            .interner
            .any(ty, &|k| matches!(k, TyKind::Projection { .. }))
        {
            return ty;
        }
        let ty = self.normalize_shallow(ty, env);
        let interner = self.interner;
        let mut children = Vec::new();
        interner.map_children(ty, &mut |t| {
            children.push(t);
            t
        });
        let normalized: Vec<Ty> = children.iter().map(|&c| self.normalize(c, env)).collect();
        let mut iter = normalized.into_iter();
        interner.map_children(ty, &mut |t| iter.next().unwrap_or(t))
    }
}
