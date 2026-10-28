//! Calls, method lookup, associated items and construction (spec §3.1–§3.3, §7.2–§7.4).

use super::InferCtxt;
use crate::items::{FnSig, Items, TraitRef};
use crate::{ArgSource, CallTarget, ExprInfo, VariantTy};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{Arg, DefId, DefKind, Expr, ExprId, Mode, Name, Package};
use loom_resolve::{DefMap, Res, is_visible};
use loom_span::{Span, TextRange};
use loom_types::{Subst, Ty, TyKind};

/// An expression that names a type or a trait rather than a value.
#[derive(Clone, Copy, Debug)]
pub(super) enum TypeExpr {
    /// A type.
    Type(Ty),
    /// A trait (for `Trait.function()`).
    Trait(DefId),
}

/// The result of method lookup.
enum MethodPick {
    Inherent { def: DefId, parent: Subst },
    Trait { method: DefId, trait_ref: TraitRef },
}

impl InferCtxt<'_> {
    /// If `expr` names a type or a trait, what it names.
    pub(super) fn type_expr(&mut self, expr: ExprId) -> Option<TypeExpr> {
        let interner = self.cx.interner;
        match self.body.expr(expr) {
            Expr::SelfType => {
                let ty = self.self_ty?;
                self.record(expr, ty);
                Some(TypeExpr::Type(ty))
            }
            Expr::Name(_) | Expr::Field { .. } => match self.res.exprs.get(&expr).copied()? {
                Res::Def(def) => match self.def_kind(def) {
                    DefKind::Struct(_) | DefKind::Enum(_) => {
                        let ty = if self.cx.items.lang.prims.contains_key(&def) {
                            self.cx.items.identity_ty(interner, def)
                        } else {
                            self.instantiate_adt(def, None).0
                        };
                        self.record(expr, ty);
                        Some(TypeExpr::Type(ty))
                    }
                    DefKind::TypeAlias(_) => {
                        let target = self.cx.items.aliases.get(&def).copied()?;
                        let generics = self.cx.items.generics_of(def);
                        let mut subst = Subst::new();
                        for &p in &generics.own {
                            let v = self.new_var();
                            subst.insert(p, v);
                        }
                        let ty = interner.subst(target, &subst);
                        self.record(expr, ty);
                        Some(TypeExpr::Type(ty))
                    }
                    DefKind::Trait(_) => {
                        self.record(expr, interner.error());
                        Some(TypeExpr::Trait(def))
                    }
                    _ => None,
                },
                Res::TyParam(param) => {
                    let ty = interner.intern(TyKind::Param(param));
                    self.record(expr, ty);
                    Some(TypeExpr::Type(ty))
                }
                Res::SelfType => {
                    let ty = self.self_ty?;
                    Some(TypeExpr::Type(ty))
                }
                _ => None,
            },
            Expr::Index {
                base,
                type_args: Some(type_args),
                ..
            } => {
                let base = *base;
                let type_args = type_args.clone();
                let TypeExpr::Type(base_ty) = self.type_expr(base)? else {
                    return None;
                };
                let args: Vec<Ty> = type_args.iter().map(|t| self.lower_type(t)).collect();
                let ty = match interner.kind(base_ty) {
                    TyKind::Adt(def, vars) if vars.len() == args.len() => {
                        interner.intern(TyKind::Adt(def, args))
                    }
                    TyKind::Adt(def, vars) => {
                        self.error(
                            Diagnostic::error(
                                codes::WRONG_GENERIC_COUNT,
                                self.span(expr),
                                format!(
                                    "`{}` takes {} generic arguments, but {} were given",
                                    self.cx.hir.def(def).name,
                                    vars.len(),
                                    args.len()
                                ),
                            )
                            .with_label("wrong number of generic arguments"),
                        );
                        interner.error()
                    }
                    _ => interner.error(),
                };
                self.record(base, ty);
                self.record(expr, ty);
                Some(TypeExpr::Type(ty))
            }
            _ => None,
        }
    }

    /// For a receiver that is still an integer or float variable: if no trait method
    /// applies, the variable takes its default type (`Int` or `Float`) so inherent methods
    /// can be found.
    fn settle_numeric_receiver(&mut self, receiver_ty: Ty, name: &Name, span: Span) -> Ty {
        let interner = self.cx.interner;
        let TyKind::Infer(_, kind) = interner.kind(receiver_ty) else {
            return receiver_ty;
        };
        let default = match kind {
            loom_types::VarKind::Int => interner.int(),
            loom_types::VarKind::Float => interner.intern(TyKind::Float(loom_types::FloatTy::F64)),
            loom_types::VarKind::General => return receiver_ty,
        };
        let snapshot = self.table.snapshot();
        let found = self.lookup_method(receiver_ty, name, span).is_some();
        self.table.rollback(snapshot);
        if !found {
            let _ = self.table.unify(interner, receiver_ty, default);
            return default;
        }
        receiver_ty
    }

    /// Instantiates a struct or enum with fresh variables, unified with `expected` if it is
    /// the same type.
    pub(super) fn instantiate_adt(&mut self, def: DefId, expected: Option<Ty>) -> (Ty, Subst) {
        let interner = self.cx.interner;
        let generics = self.cx.items.generics_of(def);
        let mut subst = Subst::new();
        let args: Vec<Ty> = generics
            .own
            .iter()
            .map(|&p| {
                let v = self.new_var();
                subst.insert(p, v);
                v
            })
            .collect();
        let ty = interner.intern(TyKind::Adt(def, args));
        if let Some(e) = expected {
            let e = self.structurally(e);
            if matches!(interner.kind(e), TyKind::Adt(d, _) if d == def) {
                let _ = self.table.unify(interner, ty, e);
            }
        }
        (ty, subst.map(|t| self.resolve(t)))
    }

    /// Instantiates function `def`'s signature, with `known` fixing some parameters (for
    /// example the receiver type's) and fresh variables for the rest.
    pub(super) fn instantiate_fn(&mut self, def: DefId, known: Option<&Subst>) -> (Subst, FnSig) {
        self.instantiate_fn_at(def, known, None)
    }

    fn instantiate_fn_at(
        &mut self,
        def: DefId,
        known: Option<&Subst>,
        span: Option<Span>,
    ) -> (Subst, FnSig) {
        let interner = self.cx.interner;
        let generics = self.cx.items.generics_of(def);
        let mut subst = known.cloned().unwrap_or_default();
        for &p in &generics.params {
            if subst.get(p).is_none() {
                let v = self.new_var();
                subst.insert(p, v);
            }
        }
        // A trait method's `Self` is a parameter too.
        if let Some(parent) = self.cx.hir.generics_parent(def)
            && matches!(self.def_kind(parent), DefKind::Trait(_))
        {
            let self_param = Items::self_param(parent);
            if subst.get(self_param).is_none() {
                let v = self.new_var();
                subst.insert(self_param, v);
            }
        }
        let sig = self.cx.items.fns.get(&def).cloned().unwrap_or(FnSig {
            receiver: None,
            params: Vec::new(),
            ret: interner.error(),
            self_ty: None,
        });
        let params = sig
            .params
            .iter()
            .map(|(mode, name, ty, default)| {
                let t = interner.subst(*ty, &subst);
                (*mode, name.clone(), self.normalize(t), *default)
            })
            .collect();
        let ret = interner.subst(sig.ret, &subst);
        let ret = self.normalize(ret);
        let self_ty = sig.self_ty.map(|t| interner.subst(t, &subst));
        if let Some(span) = span {
            for pred in &generics.predicates {
                let ty = interner.subst(pred.ty, &subst);
                let trait_ref = TraitRef {
                    trait_: pred.trait_ref.trait_,
                    args: pred
                        .trait_ref
                        .args
                        .iter()
                        .map(|&a| interner.subst(a, &subst))
                        .collect(),
                };
                self.require_trait(ty, trait_ref, span);
            }
        }
        (
            subst,
            FnSig {
                receiver: sig.receiver,
                params,
                ret,
                self_ty,
            },
        )
    }

    /// A variant named `name` of the enum that `expected` is.
    pub(super) fn variant_from_expected(
        &mut self,
        name: &Name,
        expected: Option<Ty>,
    ) -> Option<(DefId, Ty, Subst)> {
        let interner = self.cx.interner;
        let e = self.structurally(expected?);
        let TyKind::Adt(def, args) = interner.kind(e) else {
            return None;
        };
        let DefKind::Enum(data) = self.def_kind(def) else {
            return None;
        };
        let variant = data
            .variants
            .iter()
            .copied()
            .find(|&v| self.cx.hir.def(v).name == *name)?;
        Some((variant, e, self.cx.items.adt_subst(def, &args)))
    }

    // ----- calls ---------------------------------------------------------------------------

    pub(super) fn call(
        &mut self,
        id: ExprId,
        callee: ExprId,
        args: &[Arg],
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        // Unqualified variant: `Circle(radius: 1.0)` where a `Shape` is expected.
        if self.res.unresolved_exprs.contains(&callee) {
            let Expr::Name(name) = self.body.expr(callee) else {
                return interner.error();
            };
            let name = name.clone();
            if let Some((variant, enum_ty, subst)) = self.variant_from_expected(&name, expected) {
                self.record(callee, enum_ty);
                return self.construct_variant(id, variant, enum_ty, subst, args);
            }
            self.infer_args(args);
            self.record(callee, interner.error());
            let diagnostic = Diagnostic::error(
                codes::UNRESOLVED_NAME,
                self.span(callee),
                format!("cannot find `{name}` in this scope"),
            )
            .with_label("not found");
            self.error(diagnostic);
            return interner.error();
        }
        if matches!(self.body.expr(callee), Expr::SelfType) {
            if let Some(self_ty) = self.self_ty
                && let TyKind::Adt(def, _) = interner.kind(self_ty)
                && matches!(self.def_kind(def), DefKind::Struct(_))
            {
                self.record(callee, self_ty);
                return self.construct_struct(id, def, args, Some(self_ty));
            }
        }
        if let Expr::Field {
            base,
            name,
            name_range,
        } = self.body.expr(callee)
            && !matches!(self.res.exprs.get(&callee), Some(Res::Def(_)))
            && let Some(type_expr) = self.type_expr(*base)
        {
            let (name, name_range) = (name.clone(), *name_range);
            let ty = self.assoc_call(id, type_expr, &name, name_range, args, expected);
            self.record(callee, interner.error());
            return ty;
        }
        if let Some(Res::Def(def)) = self.res.exprs.get(&callee).copied() {
            match self.def_kind(def) {
                DefKind::Fn(_) => {
                    self.record(callee, interner.error());
                    let span = self.span(id);
                    let (subst, sig) = self.instantiate_fn_at(def, None, Some(span));
                    let slots = self.check_args(id, &sig.params, args);
                    self.exprs.insert(
                        id,
                        ExprInfo::Call {
                            target: CallTarget::Fn { def, subst },
                            receiver: None,
                            args: slots,
                        },
                    );
                    return sig.ret;
                }
                DefKind::Struct(_) => {
                    self.record(callee, interner.error());
                    return self.construct_struct(id, def, args, expected);
                }
                DefKind::Variant(_) => {
                    let enum_def = self.cx.hir.def(def).parent.unwrap_or(def);
                    let (enum_ty, subst) = self.instantiate_adt(enum_def, expected);
                    self.record(callee, enum_ty);
                    return self.construct_variant(id, def, enum_ty, subst, args);
                }
                _ => {}
            }
        }
        // A value of a callable type.
        let callee_ty = self.infer(callee);
        let callee_ty = self.structurally(callee_ty);
        match interner.kind(callee_ty) {
            TyKind::Callable { params, ret, .. } => {
                let params: Vec<(Mode, Name, Ty, bool)> = params
                    .iter()
                    .enumerate()
                    .map(|(i, &t)| (Mode::Borrow, Name::new(&format!("_{i}")), t, false))
                    .collect();
                for arg in args {
                    if let Some((name, range)) = &arg.name {
                        let diagnostic = Diagnostic::error(
                            codes::BAD_NAMED_ARGUMENT,
                            Span::new(self.body.file, *range),
                            format!("a closure cannot take the named argument `{name}`"),
                        )
                        .with_label("closure parameters are positional");
                        self.error(diagnostic);
                    }
                }
                let slots = self.check_args(id, &params, args);
                self.exprs.insert(
                    id,
                    ExprInfo::Call {
                        target: CallTarget::Value,
                        receiver: None,
                        args: slots,
                    },
                );
                ret
            }
            TyKind::Error => {
                self.infer_args(args);
                interner.error()
            }
            TyKind::Infer(..) => {
                self.infer_args(args);
                self.annotations_needed_at(callee)
            }
            _ => {
                self.infer_args(args);
                let text = self.show(callee_ty);
                self.error(
                    Diagnostic::error(
                        codes::NOT_CALLABLE,
                        self.span(callee),
                        format!("a value of type `{text}` cannot be called"),
                    )
                    .with_label("not a function"),
                );
                interner.error()
            }
        }
    }

    fn annotations_needed_at(&mut self, expr: ExprId) -> Ty {
        if !self.reported_annotation {
            self.reported_annotation = true;
            self.error(
                Diagnostic::error(
                    codes::TYPE_ANNOTATIONS_NEEDED,
                    self.span(expr),
                    "type annotations needed",
                )
                .with_label("the type of this value must be known here"),
            );
        }
        self.cx.interner.error()
    }

    fn infer_args(&mut self, args: &[Arg]) {
        for arg in args {
            self.infer(arg.value);
        }
    }

    /// Binds `args` to `params` (spec §3.1 rule 4) and checks them, non-closures first.
    fn check_args(
        &mut self,
        call: ExprId,
        params: &[(Mode, Name, Ty, bool)],
        args: &[Arg],
    ) -> Vec<ArgSource> {
        let mut slots: Vec<Option<usize>> = vec![None; params.len()];
        let mut extra = Vec::new();
        let mut positional = 0;
        for (i, arg) in args.iter().enumerate() {
            if arg.trailing {
                match slots.last_mut() {
                    Some(slot @ None) => *slot = Some(i),
                    _ => extra.push(i),
                }
            } else if let Some((name, range)) = &arg.name {
                match params.iter().position(|p| p.1 == *name) {
                    Some(p) if slots[p].is_none() => slots[p] = Some(i),
                    Some(_) => {
                        self.error(
                            Diagnostic::error(
                                codes::BAD_NAMED_ARGUMENT,
                                Span::new(self.body.file, *range),
                                format!("the argument `{name}` is given more than once"),
                            )
                            .with_label("repeated argument"),
                        );
                        extra.push(i);
                    }
                    None => {
                        self.error(
                            Diagnostic::error(
                                codes::BAD_NAMED_ARGUMENT,
                                Span::new(self.body.file, *range),
                                format!("there is no parameter named `{name}`"),
                            )
                            .with_label("unknown parameter"),
                        );
                        extra.push(i);
                    }
                }
            } else {
                while positional < slots.len() && slots[positional].is_some() {
                    positional += 1;
                }
                if positional < slots.len() {
                    slots[positional] = Some(i);
                    positional += 1;
                } else {
                    extra.push(i);
                }
            }
        }
        if !extra.is_empty() {
            let span = self.span(args[extra[0]].value);
            self.error(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    span,
                    format!(
                        "this call takes {} argument{}, but more were given",
                        params.len(),
                        if params.len() == 1 { "" } else { "s" }
                    ),
                )
                .with_label("unexpected argument"),
            );
            for &i in &extra {
                self.infer(args[i].value);
            }
        }
        let mut missing = Vec::new();
        for (p, slot) in slots.iter().enumerate() {
            if slot.is_none() && !params[p].3 {
                missing.push(format!("`{}`", params[p].1));
            }
        }
        if !missing.is_empty() {
            self.error(
                Diagnostic::error(
                    codes::WRONG_ARGUMENT_COUNT,
                    self.span(call),
                    format!(
                        "missing argument{} {}",
                        if missing.len() == 1 { "" } else { "s" },
                        missing.join(", ")
                    ),
                )
                .with_label("missing arguments"),
            );
        }
        // Non-closure arguments first, so closures see the solved generic parameters.
        for pass in 0..2 {
            for (p, slot) in slots.iter().enumerate() {
                let Some(i) = *slot else { continue };
                let is_closure = matches!(self.body.expr(args[i].value), Expr::Closure { .. });
                if (pass == 0) == is_closure {
                    continue;
                }
                self.check(args[i].value, params[p].2);
                if params[p].0 == Mode::Mut {
                    self.require_mutable(args[i].value, "borrow mutably");
                }
            }
        }
        slots
            .into_iter()
            .map(|s| s.map_or(ArgSource::Default, ArgSource::Arg))
            .collect()
    }

    // ----- construction ----------------------------------------------------------------------

    fn construct_struct(
        &mut self,
        id: ExprId,
        def: DefId,
        args: &[Arg],
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        if self.cx.hir.def(def).lang().is_some()
            && self.cx.hir.package_of(self.owner) != Package::Core
        {
            self.infer_args(args);
            self.error(
                Diagnostic::error(
                    codes::BAD_CONSTRUCTION_FIELDS,
                    self.span(id),
                    format!(
                        "the built-in type `{}` cannot be constructed directly",
                        self.cx.hir.def(def).name
                    ),
                )
                .with_label("use a literal or an associated function such as `.new`"),
            );
            return interner.error();
        }
        let (ty, subst) = self.instantiate_adt(def, expected);
        let fields = self.cx.items.fields.get(&def).cloned().unwrap_or_default();
        let struct_module = self.cx.hir.def(def).module;
        for field in &fields {
            if !is_visible(self.cx.hir, field.vis, struct_module, self.module) {
                self.error(
                    Diagnostic::error(
                        codes::PRIVATE_ITEM,
                        self.span(id),
                        format!(
                            "field `{}` of `{}` is private here",
                            field.name,
                            self.cx.hir.def(def).name
                        ),
                    )
                    .with_label("cannot construct a struct with private fields here"),
                );
                break;
            }
        }
        let types: Vec<(Name, Ty)> = fields
            .iter()
            .map(|f| (f.name.clone(), interner.subst(f.ty, &subst)))
            .collect();
        let slots = self.named_fields(id, &types, args, &self.cx.hir.def(def).name.to_string());
        let subst = subst.map(|t| self.resolve(t));
        self.exprs.insert(
            id,
            ExprInfo::Call {
                target: CallTarget::Struct { def, subst },
                receiver: None,
                args: slots,
            },
        );
        ty
    }

    pub(super) fn construct_variant(
        &mut self,
        id: ExprId,
        variant: DefId,
        enum_ty: Ty,
        subst: Subst,
        args: &[Arg],
    ) -> Ty {
        let interner = self.cx.interner;
        let name = self.cx.hir.def(variant).name.to_string();
        let slots = match self.cx.items.variants.get(&variant).cloned() {
            Some(VariantTy::Positional(types)) => {
                let params: Vec<(Mode, Name, Ty, bool)> = types
                    .iter()
                    .enumerate()
                    .map(|(i, &t)| {
                        (
                            Mode::Own,
                            Name::new(&format!("_{i}")),
                            interner.subst(t, &subst),
                            false,
                        )
                    })
                    .collect();
                self.check_args(id, &params, args)
            }
            Some(VariantTy::Named(fields)) => {
                let types: Vec<(Name, Ty)> = fields
                    .iter()
                    .map(|f| (f.name.clone(), interner.subst(f.ty, &subst)))
                    .collect();
                self.named_fields(id, &types, args, &name)
            }
            _ => {
                self.infer_args(args);
                self.error(
                    Diagnostic::error(
                        codes::WRONG_ARGUMENT_COUNT,
                        self.span(id),
                        format!("the unit variant `{name}` takes no arguments"),
                    )
                    .with_label(format!("write `{name}` without parentheses")),
                );
                Vec::new()
            }
        };
        let subst = subst.map(|t| self.resolve(t));
        self.exprs.insert(
            id,
            ExprInfo::Call {
                target: CallTarget::Variant {
                    def: variant,
                    subst,
                },
                receiver: None,
                args: slots,
            },
        );
        enum_ty
    }

    /// Checks `Name(field: value, …)`: every field exactly once, by name (spec §3.2 rule 2).
    fn named_fields(
        &mut self,
        id: ExprId,
        fields: &[(Name, Ty)],
        args: &[Arg],
        what: &str,
    ) -> Vec<ArgSource> {
        let mut slots: Vec<Option<usize>> = vec![None; fields.len()];
        for (i, arg) in args.iter().enumerate() {
            let Some((name, range)) = &arg.name else {
                self.infer(arg.value);
                self.error(
                    Diagnostic::error(
                        codes::BAD_CONSTRUCTION_FIELDS,
                        self.span(arg.value),
                        format!("the fields of `{what}` must be given by name"),
                    )
                    .with_label("write `field: value`"),
                );
                continue;
            };
            match fields.iter().position(|f| f.0 == *name) {
                Some(f) if slots[f].is_none() => {
                    slots[f] = Some(i);
                    self.check(arg.value, fields[f].1);
                }
                Some(_) => {
                    self.infer(arg.value);
                    self.error(
                        Diagnostic::error(
                            codes::BAD_CONSTRUCTION_FIELDS,
                            Span::new(self.body.file, *range),
                            format!("the field `{name}` is given more than once"),
                        )
                        .with_label("repeated field"),
                    );
                }
                None => {
                    self.infer(arg.value);
                    self.error(
                        Diagnostic::error(
                            codes::UNKNOWN_FIELD,
                            Span::new(self.body.file, *range),
                            format!("`{what}` has no field named `{name}`"),
                        )
                        .with_label("unknown field"),
                    );
                }
            }
        }
        let missing: Vec<String> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_none())
            .map(|(f, _)| format!("`{}`", fields[f].0))
            .collect();
        if !missing.is_empty() {
            self.error(
                Diagnostic::error(
                    codes::BAD_CONSTRUCTION_FIELDS,
                    self.span(id),
                    format!(
                        "missing field{} {} in `{what}`",
                        if missing.len() == 1 { "" } else { "s" },
                        missing.join(", ")
                    ),
                )
                .with_label("every field must be given"),
            );
        }
        slots
            .into_iter()
            .map(|s| s.map_or(ArgSource::Default, ArgSource::Arg))
            .collect()
    }

    // ----- associated items ------------------------------------------------------------------

    /// `Type.name` without a call: a unit variant, a constant, or a function value.
    pub(super) fn assoc_value(
        &mut self,
        id: ExprId,
        type_expr: TypeExpr,
        name: &Name,
        name_range: TextRange,
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        let TypeExpr::Type(ty) = type_expr else {
            return self.not_a_value(id, "a trait");
        };
        let ty = self.structurally(ty);
        if let TyKind::Adt(def, args) = interner.kind(ty)
            && let DefKind::Enum(data) = self.def_kind(def)
            && let Some(&variant) = data
                .variants
                .iter()
                .find(|&&v| self.cx.hir.def(v).name == *name)
        {
            let _ = expected;
            let subst = self.cx.items.adt_subst(def, &args);
            return self.variant_value(id, variant, ty, &subst);
        }
        if let Some(def) = self.type_def(ty)
            && let Some(&member) = self.cx.items.inherent.get(&def).and_then(|m| m.get(name))
        {
            match self.def_kind(member) {
                DefKind::Const(_) => {
                    self.exprs.insert(id, ExprInfo::Const { def: member });
                    let ty = self
                        .cx
                        .items
                        .consts
                        .get(&member)
                        .copied()
                        .unwrap_or_else(|| interner.error());
                    return self.normalize(ty);
                }
                // `Type.name` calls an associated function without arguments (uniform
                // access, spec §7.4 rule 1).
                _ => {}
            }
        }
        // `Integer.parse` written without a call is a zero-argument associated call.
        self.assoc_call(id, type_expr, name, name_range, &[], expected)
    }

    /// `Type.name(args)`: a variant constructor or an associated function.
    pub(super) fn assoc_call(
        &mut self,
        id: ExprId,
        type_expr: TypeExpr,
        name: &Name,
        name_range: TextRange,
        args: &[Arg],
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        let span = Span::new(self.body.file, name_range);
        let (self_ty, traits) = match type_expr {
            TypeExpr::Trait(trait_) => {
                let v = self.new_var();
                (v, vec![trait_])
            }
            TypeExpr::Type(ty) => {
                let ty = self.structurally(ty);
                if let TyKind::Adt(def, adt_args) = interner.kind(ty)
                    && let DefKind::Enum(data) = self.def_kind(def)
                    && let Some(&variant) = data
                        .variants
                        .iter()
                        .find(|&&v| self.cx.hir.def(v).name == *name)
                {
                    let subst = self.cx.items.adt_subst(def, &adt_args);
                    let enum_ty = ty;
                    if let Some(e) = expected {
                        let _ = self.table.unify(interner, enum_ty, e);
                    }
                    return self.construct_variant(id, variant, enum_ty, subst, args);
                }
                if let Some(def) = self.type_def(ty)
                    && let Some(&member) =
                        self.cx.items.inherent.get(&def).and_then(|m| m.get(name))
                    && let DefKind::Fn(f) = self.def_kind(member)
                {
                    if f.receiver.is_some() {
                        self.infer_args(args);
                        self.error(
                            Diagnostic::error(
                                codes::UNKNOWN_ASSOCIATED_ITEM,
                                span,
                                format!("`{name}` is a method; call it on a value"),
                            )
                            .with_label("not an associated function"),
                        );
                        return interner.error();
                    }
                    self.check_member_visible(member, span);
                    let parent = match interner.kind(ty) {
                        TyKind::Adt(d, a) => self.cx.items.adt_subst(d, &a),
                        _ => Subst::new(),
                    };
                    let (subst, sig) =
                        self.instantiate_fn_at(member, Some(&parent), Some(self.span(id)));
                    let slots = self.check_args(id, &sig.params, args);
                    self.exprs.insert(
                        id,
                        ExprInfo::Call {
                            target: CallTarget::Fn { def: member, subst },
                            receiver: None,
                            args: slots,
                        },
                    );
                    return sig.ret;
                }
                (ty, self.candidate_traits(ty))
            }
        };
        // A trait's associated function, with `Self` = `self_ty`.
        let mut found = Vec::new();
        for trait_ in traits {
            let Some(&method) = self
                .cx
                .items
                .traits
                .get(&trait_)
                .and_then(|t| t.methods.get(name))
            else {
                continue;
            };
            let trait_ref = self.fresh_trait_ref(trait_);
            if matches!(type_expr, TypeExpr::Trait(_)) || self.implements(self_ty, &trait_ref) {
                found.push((method, trait_ref));
            }
        }
        found.dedup_by_key(|(m, _)| *m);
        match found.as_slice() {
            [(method, trait_ref)] => {
                let (method, trait_ref) = (*method, trait_ref.clone());
                self.require_trait(self_ty, trait_ref.clone(), span);
                let subst = self.trait_subst(method, &trait_ref, self_ty);
                let (subst, sig) =
                    self.instantiate_fn_at(method, Some(&subst), Some(self.span(id)));
                let slots = self.check_args(id, &sig.params, args);
                if let Some(e) = expected
                    && sig.receiver.is_none()
                {
                    let _ = e;
                }
                self.exprs.insert(
                    id,
                    ExprInfo::Call {
                        target: CallTarget::Trait {
                            method,
                            self_ty,
                            subst,
                        },
                        receiver: None,
                        args: slots,
                    },
                );
                sig.ret
            }
            [] => {
                self.infer_args(args);
                let text = match type_expr {
                    TypeExpr::Type(t) => self.show(t),
                    TypeExpr::Trait(t) => self.cx.hir.def(t).name.to_string(),
                };
                self.error(
                    Diagnostic::error(
                        codes::UNKNOWN_ASSOCIATED_ITEM,
                        span,
                        format!("no associated item named `{name}` for `{text}`"),
                    )
                    .with_label("not found"),
                );
                interner.error()
            }
            _ => {
                self.infer_args(args);
                self.error(
                    Diagnostic::error(
                        codes::AMBIGUOUS_METHOD,
                        span,
                        format!("`{name}` is provided by more than one trait"),
                    )
                    .with_label("ambiguous"),
                );
                interner.error()
            }
        }
    }

    fn fresh_trait_ref(&mut self, trait_: DefId) -> TraitRef {
        let count = self.cx.items.generics_of(trait_).own.len();
        let args = (0..count).map(|_| self.new_var()).collect();
        TraitRef { trait_, args }
    }

    /// The substitution of a trait method's trait-level parameters.
    fn trait_subst(&mut self, method: DefId, trait_ref: &TraitRef, self_ty: Ty) -> Subst {
        let _ = method;
        let mut subst = Subst::new();
        subst.insert(Items::self_param(trait_ref.trait_), self_ty);
        for (&p, &a) in self
            .cx
            .items
            .generics_of(trait_ref.trait_)
            .own
            .iter()
            .zip(&trait_ref.args)
        {
            subst.insert(p, a);
        }
        subst
    }

    /// The definition carrying the inherent members of `ty`.
    fn type_def(&self, ty: Ty) -> Option<DefId> {
        match self.cx.interner.kind(ty) {
            TyKind::Adt(def, _) => Some(def),
            kind => self.cx.items.prim_def(&kind),
        }
    }

    /// Traits whose methods may apply to a value of type `ty`: those in scope, those
    /// bounding it, and a trait object's own trait.
    fn candidate_traits(&self, ty: Ty) -> Vec<DefId> {
        let mut traits = self.cx.def_map.traits_in_scope(self.cx.hir, self.module);
        for pred in &self.env.predicates {
            if self.resolve(pred.ty) == ty && !traits.contains(&pred.trait_ref.trait_) {
                traits.push(pred.trait_ref.trait_);
            }
        }
        if let TyKind::Dyn(def, _) = self.cx.interner.kind(ty) {
            let mut stack = vec![def];
            while let Some(t) = stack.pop() {
                if !traits.contains(&t) {
                    traits.push(t);
                }
                if let Some(info) = self.cx.items.traits.get(&t) {
                    stack.extend(info.supertraits.iter().map(|s| s.trait_));
                }
            }
        }
        traits
    }

    fn check_member_visible(&mut self, member: DefId, span: Span) {
        if !DefMap::def_visible(self.cx.hir, member, self.module) {
            let data = self.cx.hir.def(member);
            self.error(
                Diagnostic::error(
                    codes::PRIVATE_ITEM,
                    span,
                    format!("`{}` is private", data.name),
                )
                .with_label("not visible here")
                .with_secondary(data.span, "declared here without `pub`"),
            );
        }
    }

    // ----- methods ---------------------------------------------------------------------------

    fn lookup_method(&mut self, receiver_ty: Ty, name: &Name, span: Span) -> Option<MethodPick> {
        let interner = self.cx.interner;
        if let Some(def) = self.type_def(receiver_ty)
            && let Some(&member) = self.cx.items.inherent.get(&def).and_then(|m| m.get(name))
            && matches!(self.def_kind(member), DefKind::Fn(f) if f.receiver.is_some())
        {
            let parent = match interner.kind(receiver_ty) {
                TyKind::Adt(d, a) => self.cx.items.adt_subst(d, &a),
                _ => Subst::new(),
            };
            return Some(MethodPick::Inherent {
                def: member,
                parent,
            });
        }
        let mut found: Vec<(DefId, TraitRef)> = Vec::new();
        for trait_ in self.candidate_traits(receiver_ty) {
            let Some(&method) = self
                .cx
                .items
                .traits
                .get(&trait_)
                .and_then(|t| t.methods.get(name))
            else {
                continue;
            };
            if !matches!(self.def_kind(method), DefKind::Fn(f) if f.receiver.is_some()) {
                continue;
            }
            let trait_ref = self.fresh_trait_ref(trait_);
            if self.implements(receiver_ty, &trait_ref) && !found.iter().any(|(m, _)| *m == method)
            {
                found.push((method, trait_ref));
            }
        }
        match found.len() {
            0 => None,
            1 => {
                let (method, trait_ref) = found.remove(0);
                Some(MethodPick::Trait { method, trait_ref })
            }
            _ => {
                let names: Vec<String> = found
                    .iter()
                    .map(|(_, t)| format!("`{}`", self.cx.hir.def(t.trait_).name))
                    .collect();
                self.error(
                    Diagnostic::error(
                        codes::AMBIGUOUS_METHOD,
                        span,
                        format!("method `{name}` is ambiguous"),
                    )
                    .with_label(format!("provided by {}", names.join(" and "))),
                );
                let (method, trait_ref) = found.remove(0);
                Some(MethodPick::Trait { method, trait_ref })
            }
        }
    }

    #[allow(clippy::too_many_arguments, reason = "a method call has many parts")]
    pub(super) fn method_call(
        &mut self,
        id: ExprId,
        receiver: ExprId,
        receiver_ty: Ty,
        name: &Name,
        name_range: TextRange,
        args: &[Arg],
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        let _ = expected;
        let span = Span::new(self.body.file, name_range);
        let receiver_ty = self.structurally(receiver_ty);
        let receiver_ty = self.settle_numeric_receiver(receiver_ty, name, span);
        match interner.kind(receiver_ty) {
            TyKind::Error => {
                self.infer_args(args);
                return interner.error();
            }
            TyKind::Infer(_, loom_types::VarKind::General) => {
                self.infer_args(args);
                return self.annotations_needed_at(receiver);
            }
            _ => {}
        }
        let Some(pick) = self.lookup_method(receiver_ty, name, span) else {
            self.infer_args(args);
            let text = self.show(receiver_ty);
            let mut diagnostic = Diagnostic::error(
                codes::NO_SUCH_METHOD,
                span,
                format!("no method named `{name}` for `{text}`"),
            )
            .with_label("method not found");
            if self.field_index(receiver_ty, name).is_some() {
                diagnostic = diagnostic.with_help(format!(
                    "to call the function stored in the field, write `(value.{name})(…)`"
                ));
            }
            self.error(diagnostic);
            return interner.error();
        };
        self.finish_method_call(id, receiver, receiver_ty, pick, args, span)
    }

    fn finish_method_call(
        &mut self,
        id: ExprId,
        receiver: ExprId,
        receiver_ty: Ty,
        pick: MethodPick,
        args: &[Arg],
        span: Span,
    ) -> Ty {
        let call_span = self.span(id);
        let (target, sig) = match pick {
            MethodPick::Inherent { def, parent } => {
                self.check_member_visible(def, span);
                let (subst, sig) = self.instantiate_fn_at(def, Some(&parent), Some(call_span));
                (CallTarget::Fn { def, subst }, sig)
            }
            MethodPick::Trait { method, trait_ref } => {
                self.require_trait(receiver_ty, trait_ref.clone(), span);
                let known = self.trait_subst(method, &trait_ref, receiver_ty);
                let (subst, sig) = self.instantiate_fn_at(method, Some(&known), Some(call_span));
                (
                    CallTarget::Trait {
                        method,
                        self_ty: receiver_ty,
                        subst,
                    },
                    sig,
                )
            }
        };
        if sig.receiver == Some(Mode::Mut) {
            self.require_mutable(receiver, "borrow mutably");
        }
        let slots = self.check_args(id, &sig.params, args);
        self.exprs.insert(
            id,
            ExprInfo::Call {
                target,
                receiver: Some(receiver),
                args: slots,
            },
        );
        sig.ret
    }

    fn field_index(&self, ty: Ty, name: &Name) -> Option<usize> {
        let TyKind::Adt(def, _) = self.cx.interner.kind(ty) else {
            return None;
        };
        self.cx
            .items
            .fields
            .get(&def)?
            .iter()
            .position(|f| f.name == *name)
    }

    /// `base.name`: a field, or a method called without arguments (spec §7.4 rule 1).
    pub(super) fn field_or_method(
        &mut self,
        id: ExprId,
        base: ExprId,
        base_ty: Ty,
        name: &Name,
        name_range: TextRange,
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        let span = Span::new(self.body.file, name_range);
        let base_ty = self.structurally(base_ty);
        let base_ty = self.settle_numeric_receiver(base_ty, name, span);
        match interner.kind(base_ty) {
            TyKind::Error => return interner.error(),
            TyKind::Infer(_, loom_types::VarKind::General) => {
                return self.annotations_needed_at(base);
            }
            _ => {}
        }
        if let TyKind::Adt(def, args) = interner.kind(base_ty)
            && let Some(index) = self.field_index(base_ty, name)
        {
            let field = self.cx.items.fields[&def][index].clone();
            if is_visible(
                self.cx.hir,
                field.vis,
                self.cx.hir.def(def).module,
                self.module,
            ) {
                self.exprs.insert(id, ExprInfo::Field(index));
                let subst = self.cx.items.adt_subst(def, &args);
                let ty = interner.subst(field.ty, &subst);
                return self.normalize(ty);
            }
            if self.lookup_method(base_ty, name, span).is_none() {
                self.error(
                    Diagnostic::error(
                        codes::PRIVATE_ITEM,
                        span,
                        format!(
                            "field `{name}` of `{}` is private",
                            self.cx.hir.def(def).name
                        ),
                    )
                    .with_label("private field"),
                );
                return interner.error();
            }
        }
        match self.lookup_method(base_ty, name, span) {
            Some(pick) => self.finish_method_call(id, base, base_ty, pick, &[], span),
            None => {
                let _ = expected;
                let text = self.show(base_ty);
                let is_struct = matches!(interner.kind(base_ty), TyKind::Adt(def, _) if self.cx.items.fields.get(&def).is_some_and(|f| !f.is_empty()));
                let code = if is_struct {
                    codes::UNKNOWN_FIELD
                } else {
                    codes::NO_SUCH_METHOD
                };
                self.error(
                    Diagnostic::error(
                        code,
                        span,
                        format!("no field or method named `{name}` on `{text}`"),
                    )
                    .with_label("unknown field or method"),
                );
                interner.error()
            }
        }
    }
}
