//! Inference of one body (spec §6): bidirectional checking with unification.

mod call;
mod expr;
mod pat;

use crate::display::ty_to_string;
use crate::exhaustive;
use crate::items::{Predicate, TraitRef};
use crate::lower::TyLowerer;
use crate::traits::{Env, SelectError, Solver};
use crate::{Coercion, Ctx, ExprInfo, PatInfo, TypedBody};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{BindingId, Body, BodyId, DefId, DefKind, ExprId, FnKind, Mode, PatId, TypeRef};
use loom_resolve::BodyResolution;
use loom_span::Span;
use loom_types::{InferTable, IntTy, Subst, Ty, TyKind, VarKind};
use std::collections::{HashMap, HashSet};

/// A closure being checked: the bindings declared inside it and whether it mutates a
/// captured binding.
struct ClosureFrame {
    declared: HashSet<BindingId>,
    mutates_capture: bool,
}

/// A loop being checked: the type of its `break` values (`loop` only) and whether it
/// has a `break`.
struct LoopFrame {
    break_ty: Option<Ty>,
    has_break: bool,
}

/// An obligation checked at the end of the body.
struct Obligation {
    pred: Predicate,
    span: Span,
}

/// An operator whose operand types must be validated once inference is done.
struct OperatorCheck {
    expr: ExprId,
    ty: Ty,
    op: &'static str,
    allowed: Allowed,
}

#[derive(Clone, Copy)]
enum Allowed {
    /// Integers and floats.
    Numeric,
    /// Signed integers and floats.
    Signed,
    /// Integers.
    Integer,
    /// Integers and `Bool`.
    IntegerOrBool,
    /// Integers and `Char`.
    IntegerOrChar,
}

pub(crate) struct InferCtxt<'a> {
    cx: &'a Ctx<'a>,
    body: &'a Body,
    res: &'a BodyResolution,
    owner: DefId,
    module: DefId,
    table: InferTable,
    env: Env,
    expr_tys: Vec<Option<Ty>>,
    pat_tys: Vec<Option<Ty>>,
    binding_tys: Vec<Option<Ty>>,
    exprs: HashMap<ExprId, ExprInfo>,
    pats: HashMap<PatId, PatInfo>,
    coercions: HashMap<ExprId, Coercion>,
    /// Inside a pattern matched against a `ref` value: bindings borrow (spec §8.3 rule 6).
    binding_ref: Option<bool>,
    return_tys: Vec<Ty>,
    loops: Vec<LoopFrame>,
    closures: Vec<ClosureFrame>,
    obligations: Vec<Obligation>,
    operator_checks: Vec<OperatorCheck>,
    int_literals: Vec<(ExprId, u128, bool)>,
    cases: Vec<ExprId>,
    irrefutable: Vec<(PatId, &'static str)>,
    self_ty: Option<Ty>,
    receiver: Option<Mode>,
    diagnostics: Vec<Diagnostic>,
    reported_annotation: bool,
}

pub(crate) fn infer_body(cx: &Ctx<'_>, id: BodyId, diagnostics: &mut Vec<Diagnostic>) -> TypedBody {
    let body = cx.hir.body(id);
    let res = cx.resolution.body(id);
    let owner = body.owner;
    let generics = cx.items.generics_of(owner);
    let mut table = InferTable::new();
    let env = Solver {
        hir: cx.hir,
        items: cx.items,
        interner: cx.interner,
        table: &mut table,
    }
    .elaborate(&generics.predicates);
    let sig = cx.items.fns.get(&owner);
    let mut icx = InferCtxt {
        cx,
        body,
        res,
        owner,
        module: cx.hir.def(owner).module,
        table,
        env,
        expr_tys: vec![None; body.exprs.len()],
        pat_tys: vec![None; body.pats.len()],
        binding_tys: vec![None; body.bindings.len()],
        exprs: HashMap::new(),
        pats: HashMap::new(),
        coercions: HashMap::new(),
        binding_ref: None,
        return_tys: Vec::new(),
        loops: Vec::new(),
        closures: Vec::new(),
        obligations: Vec::new(),
        operator_checks: Vec::new(),
        int_literals: Vec::new(),
        cases: Vec::new(),
        irrefutable: Vec::new(),
        self_ty: sig.and_then(|s| s.self_ty),
        receiver: sig.and_then(|s| s.receiver),
        diagnostics: Vec::new(),
        reported_annotation: false,
    };
    icx.run(sig.cloned());
    let (typed, mut found) = icx.finish();
    diagnostics.append(&mut found);
    typed
}

impl<'a> InferCtxt<'a> {
    fn interner(&self) -> &'a loom_types::Interner {
        self.cx.interner
    }

    fn run(&mut self, sig: Option<crate::FnSig>) {
        if let Some(sig) = sig {
            let ret = self.normalize(sig.ret);
            for (i, &binding) in self.body.params.iter().enumerate() {
                let ty = sig
                    .params
                    .get(i)
                    .map_or_else(|| self.interner().error(), |p| p.2);
                let ty = self.normalize(ty);
                self.binding_tys[binding.index()] = Some(ty);
            }
            for (i, default) in self.body.defaults.clone().into_iter().enumerate() {
                if let Some(default) = default {
                    let ty = self.binding_tys[self.body.params[i].index()]
                        .unwrap_or_else(|| self.interner().error());
                    self.check(default, ty);
                }
            }
            if let Some(root) = self.body.root {
                self.return_tys.push(ret);
                self.check_block_value(root, ret);
                self.return_tys.pop();
            }
        } else {
            // A constant.
            let ty = self
                .cx
                .items
                .consts
                .get(&self.owner)
                .copied()
                .unwrap_or_else(|| self.interner().error());
            if let Some(root) = self.body.root {
                self.return_tys.push(ty);
                self.check(root, ty);
                self.return_tys.pop();
            }
        }
    }

    /// Checks a body or branch whose value becomes `expected`, except that a `Unit`
    /// expectation discards the value (spec §3.1 rule 1).
    fn check_block_value(&mut self, expr: ExprId, expected: Ty) -> Ty {
        let resolved = self.resolve(expected);
        if matches!(self.interner().kind(resolved), TyKind::Unit) {
            self.infer(expr);
            expected
        } else {
            self.check(expr, expected)
        }
    }

    // ----- small helpers -------------------------------------------------------------------

    fn span(&self, expr: ExprId) -> Span {
        self.body.expr_span(expr)
    }

    fn new_var(&mut self) -> Ty {
        self.table.new_var(self.cx.interner, VarKind::General)
    }

    fn resolve(&self, ty: Ty) -> Ty {
        self.table.resolve(self.cx.interner, ty)
    }

    fn shallow(&self, ty: Ty) -> Ty {
        self.table.shallow(self.cx.interner, ty)
    }

    fn solver(&mut self) -> Solver<'_> {
        Solver {
            hir: self.cx.hir,
            items: self.cx.items,
            interner: self.cx.interner,
            table: &mut self.table,
        }
    }

    fn normalize(&mut self, ty: Ty) -> Ty {
        let env = std::mem::take(&mut self.env);
        let result = self.solver().normalize(ty, &env);
        self.env = env;
        result
    }

    /// The resolved, normalized shape of `ty` with outer `ref` layers removed: what an
    /// operator, method call, field access or pattern sees (spec §8.3 rule 2).
    fn deref(&mut self, ty: Ty) -> Ty {
        let ty = self.structurally(ty);
        self.cx.interner.strip_refs(ty)
    }

    /// The resolved, normalized shape of `ty`.
    fn structurally(&mut self, ty: Ty) -> Ty {
        let ty = self.resolve(ty);
        self.normalize(ty)
    }

    fn show(&self, ty: Ty) -> String {
        let ty = self.resolve(ty);
        ty_to_string(self.cx.hir, self.cx.items, self.cx.interner, ty)
    }

    fn record(&mut self, expr: ExprId, ty: Ty) -> Ty {
        self.expr_tys[expr.index()] = Some(ty);
        ty
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    fn lowerer(&self) -> TyLowerer<'a> {
        TyLowerer::new(
            self.cx.hir,
            self.cx.def_map,
            self.cx.items,
            self.cx.interner,
            self.owner,
        )
    }

    fn lower_type(&mut self, ty: &TypeRef) -> Ty {
        let lowered = self.lowerer().lower(ty, &mut self.diagnostics);
        self.normalize(lowered)
    }

    fn lang(&self, name: &str) -> Option<DefId> {
        self.cx.items.lang.get(name)
    }

    fn lang_adt(&self, name: &str, args: Vec<Ty>) -> Ty {
        match self.lang(name) {
            Some(def) => self.interner().intern(TyKind::Adt(def, args)),
            None => self.interner().error(),
        }
    }

    fn require(&mut self, ty: Ty, trait_name: &str, args: Vec<Ty>, span: Span) {
        if let Some(trait_) = self.lang(trait_name) {
            self.obligations.push(Obligation {
                pred: Predicate {
                    ty,
                    trait_ref: TraitRef { trait_, args },
                },
                span,
            });
        }
    }

    fn require_trait(&mut self, ty: Ty, trait_ref: TraitRef, span: Span) {
        self.obligations.push(Obligation {
            pred: Predicate { ty, trait_ref },
            span,
        });
    }

    /// Whether `ty: trait` holds or may hold.
    fn implements(&mut self, ty: Ty, trait_ref: &TraitRef) -> bool {
        let env = std::mem::take(&mut self.env);
        let snapshot = self.table.snapshot();
        let result = self.solver().select(ty, trait_ref, &env);
        self.env = env;
        self.table.rollback(snapshot);
        !matches!(result, Err(SelectError::NoImpl))
    }

    /// Unifies `actual` with `expected`, reporting E0300 at `expr` on failure.
    fn unify_at(&mut self, expr: ExprId, actual: Ty, expected: Ty) -> bool {
        if self.table.unify(self.cx.interner, actual, expected).is_ok() {
            return true;
        }
        let a = self.normalize(actual);
        let e = self.normalize(expected);
        if self.table.unify(self.cx.interner, a, e).is_ok() {
            return true;
        }
        self.mismatch(self.span(expr), expected, actual);
        false
    }

    fn mismatch(&mut self, span: Span, expected: Ty, actual: Ty) {
        let expected_text = self.show(expected);
        let actual_text = self.show(actual);
        self.error(
            Diagnostic::error(
                codes::MISMATCHED_TYPES,
                span,
                format!("mismatched types: expected `{expected_text}`, found `{actual_text}`"),
            )
            .with_label(format!("expected `{expected_text}`")),
        );
    }

    /// Checks `expr` against `expected`, applying the coercions of spec §6.5.
    fn check(&mut self, expr: ExprId, expected: Ty) -> Ty {
        let actual = self.expr(expr, Some(expected));
        self.coerce(expr, actual, expected)
    }

    /// Infers the type of `expr` without an expectation.
    fn infer(&mut self, expr: ExprId) -> Ty {
        self.expr(expr, None)
    }

    fn coerce(&mut self, expr: ExprId, actual: Ty, expected: Ty) -> Ty {
        let interner = self.cx.interner;
        let mut a = self.structurally(actual);
        let e = self.structurally(expected);
        // `ref T` where a `T` is expected, and a place where a `ref T` is expected
        // (spec §8.3 rule 2). Ownership rules on these are the borrow checker's.
        match (interner.kind(a), interner.kind(e)) {
            (TyKind::Ref { .. }, other)
                if !matches!(
                    other,
                    TyKind::Ref { .. } | TyKind::Infer(_, VarKind::General) | TyKind::Error
                ) =>
            {
                a = interner.strip_refs(a);
                self.coercions.insert(expr, Coercion::Deref);
            }
            (other, TyKind::Ref { inner, .. })
                if !matches!(
                    other,
                    TyKind::Ref { .. } | TyKind::Never | TyKind::Infer(..) | TyKind::Error
                ) =>
            {
                self.coercions.insert(expr, Coercion::Autoref);
                self.unify_at(expr, a, inner);
                return expected;
            }
            _ => {}
        }
        match (interner.kind(a), interner.kind(e)) {
            (TyKind::Never, _) => {
                self.record(expr, expected);
                expected
            }
            (TyKind::Dyn(..), TyKind::Dyn(..))
            | (TyKind::Infer(..), _)
            | (_, TyKind::Infer(..)) => {
                self.unify_at(expr, a, e);
                expected
            }
            (_, TyKind::Dyn(trait_, args)) => {
                let trait_ref = TraitRef { trait_, args };
                if !matches!(interner.kind(a), TyKind::Error) {
                    if self.implements(a, &trait_ref) {
                        self.coercions.insert(expr, Coercion::Unsize(expected));
                        self.require_trait(a, trait_ref, self.span(expr));
                    } else {
                        self.mismatch(self.span(expr), expected, actual);
                    }
                }
                expected
            }
            (
                TyKind::Callable {
                    kind: ka,
                    params: pa,
                    ret: ra,
                },
                TyKind::Callable {
                    kind: ke,
                    params: pe,
                    ret: re,
                },
            ) if ka <= ke => {
                let ok = pa.len() == pe.len()
                    && pa
                        .iter()
                        .zip(&pe)
                        .all(|(&x, &y)| self.table.unify(interner, x, y).is_ok())
                    && self.table.unify(interner, ra, re).is_ok();
                if !ok {
                    self.mismatch(self.span(expr), expected, actual);
                }
                expected
            }
            _ => {
                self.unify_at(expr, a, e);
                expected
            }
        }
    }

    // ----- bindings and mutation ---------------------------------------------------------

    fn binding_ty(&mut self, binding: BindingId) -> Ty {
        let binding = self.res.canonical(binding);
        if let Some(ty) = self.binding_tys[binding.index()] {
            ty
        } else {
            let var = self.new_var();
            self.binding_tys[binding.index()] = Some(var);
            var
        }
    }

    fn declare_binding(&mut self, binding: BindingId, ty: Ty) {
        let canonical = self.res.canonical(binding);
        if canonical == binding {
            self.binding_tys[binding.index()] = Some(ty);
        } else {
            let existing = self.binding_ty(canonical);
            let _ = self.table.unify(self.cx.interner, existing, ty);
            self.binding_tys[binding.index()] = Some(ty);
        }
        if let Some(frame) = self.closures.last_mut() {
            frame.declared.insert(binding);
        }
    }

    /// Notes that binding `b` is mutated, which makes enclosing closures that capture it
    /// `FnMut` (spec §7.9 rule 3).
    fn note_mutation(&mut self, binding: BindingId) {
        for frame in self.closures.iter_mut().rev() {
            if frame.declared.contains(&binding) {
                break;
            }
            frame.mutates_capture = true;
        }
    }

    /// The binding at the root of a place expression, and whether the place is mutable.
    /// `None` for temporaries (which may always be mutated).
    fn place_root(&self, expr: ExprId) -> Option<(Option<BindingId>, bool)> {
        match self.body.expr(expr) {
            loom_hir::Expr::Name(_) => match self.res.exprs.get(&expr) {
                Some(loom_resolve::Res::Local(b)) => {
                    let b = *b;
                    Some((Some(b), self.body.binding(b).kind.is_mutable()))
                }
                _ => None,
            },
            loom_hir::Expr::SelfValue => Some((None, self.receiver == Some(Mode::Mut))),
            loom_hir::Expr::Field { base, .. }
            | loom_hir::Expr::TupleField { base, .. }
            | loom_hir::Expr::Index { base, .. } => {
                if matches!(self.exprs.get(&expr), Some(ExprInfo::Call { .. })) {
                    return None;
                }
                self.place_root(*base)
            }
            _ => None,
        }
    }

    /// Requires `expr` to be a mutable place if it is a place (spec §4.2 rule 1).
    fn require_mutable(&mut self, expr: ExprId, what: &str) {
        if let Some((binding, mutable)) = self.place_root(expr) {
            if let Some(b) = binding {
                self.note_mutation(b);
            }
            if !mutable {
                let name = binding.map_or_else(
                    || "self".to_owned(),
                    |b| self.body.binding(b).name.to_string(),
                );
                let mut diagnostic = Diagnostic::error(
                    codes::MUTATE_IMMUTABLE,
                    self.span(expr),
                    format!("cannot {what} `{name}`, which is not mutable"),
                )
                .with_label(format!("cannot {what}"));
                diagnostic = match binding.map(|b| self.body.binding(b).kind) {
                    Some(loom_hir::BindingKind::Let) => {
                        diagnostic.with_help(format!("declare it with `var {name}`"))
                    }
                    Some(loom_hir::BindingKind::Param(_)) => {
                        diagnostic.with_help(format!("declare the parameter as `mut {name}`"))
                    }
                    None => diagnostic.with_help("declare the method with `def mut`"),
                    _ => diagnostic,
                };
                self.error(diagnostic);
            }
        }
    }

    // ----- finishing -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one case per language construct")]
    fn finish(mut self) -> (TypedBody, Vec<Diagnostic>) {
        let interner = self.cx.interner;
        // Default numeric variables (spec §6.2 rule 2).
        let unsolved: Vec<_> = self.table.unsolved().collect();
        for (var, kind) in unsolved {
            match kind {
                VarKind::Int => self.table.bind(var, interner.int()),
                VarKind::Float => self.table.bind(
                    var,
                    interner.intern(TyKind::Float(loom_types::FloatTy::F64)),
                ),
                VarKind::General => {}
            }
        }
        self.check_operators();
        self.check_obligations();
        self.check_literals();
        // Every type must now be known (spec §6.2 rule 3).
        for i in 0..self.binding_tys.len() {
            if let Some(ty) = self.binding_tys[i] {
                let ty = self.resolve(ty);
                if interner.has_infer(ty) && !self.reported_annotation {
                    self.reported_annotation = true;
                    let binding = &self.body.bindings[i];
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_ANNOTATIONS_NEEDED,
                            Span::new(self.body.file, binding.range),
                            format!("type annotations needed for `{}`", binding.name),
                        )
                        .with_label(format!("type is `{}`", self.show(ty))),
                    );
                }
            }
        }
        for i in 0..self.expr_tys.len() {
            if let Some(ty) = self.expr_tys[i] {
                let ty = self.resolve(ty);
                if interner.has_infer(ty) && !self.reported_annotation {
                    self.reported_annotation = true;
                    let id = ExprId(u32::try_from(i).unwrap_or(u32::MAX));
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::TYPE_ANNOTATIONS_NEEDED,
                            self.span(id),
                            "type annotations needed",
                        )
                        .with_label(format!("type is `{}`", self.show(ty))),
                    );
                }
            }
        }
        let has_errors = self.diagnostics.iter().any(Diagnostic::is_error);
        let finalize = |icx: &mut Self, ty: Ty| {
            let ty = icx.resolve(ty);
            let ty = icx.normalize(ty);
            if interner.has_infer(ty) {
                interner.error()
            } else {
                ty
            }
        };
        let expr_tys: Vec<Ty> = (0..self.expr_tys.len())
            .map(|i| {
                let t = self.expr_tys[i].unwrap_or_else(|| interner.error());
                finalize(&mut self, t)
            })
            .collect();
        let pat_tys: Vec<Ty> = (0..self.pat_tys.len())
            .map(|i| {
                let t = self.pat_tys[i].unwrap_or_else(|| interner.error());
                finalize(&mut self, t)
            })
            .collect();
        let binding_tys: Vec<Ty> = (0..self.binding_tys.len())
            .map(|i| {
                let canonical = self
                    .res
                    .canonical(BindingId(u32::try_from(i).unwrap_or(u32::MAX)));
                let t = self.binding_tys[canonical.index()].unwrap_or_else(|| interner.error());
                finalize(&mut self, t)
            })
            .collect();
        let infos: Vec<(ExprId, ExprInfo)> = self.exprs.drain().collect();
        let mut exprs = HashMap::new();
        for (id, info) in infos {
            let info = self.finalize_info(info);
            exprs.insert(id, info);
        }
        if !has_errors {
            let cases = std::mem::take(&mut self.cases);
            let irrefutable = std::mem::take(&mut self.irrefutable);
            let typed_view = exhaustive::View {
                hir: self.cx.hir,
                items: self.cx.items,
                interner,
                body: self.body,
                pat_tys: &pat_tys,
                pats: &self.pats,
            };
            exhaustive::check_cases(&typed_view, &cases, &irrefutable, &mut self.diagnostics);
        }
        let typed = TypedBody {
            expr_tys,
            pat_tys,
            binding_tys,
            exprs,
            pats: std::mem::take(&mut self.pats),
            coercions: std::mem::take(&mut self.coercions),
            assignments: self.res.assignments.clone(),
            aliases: self.res.aliases.clone(),
        };
        (typed, self.diagnostics)
    }

    fn finalize_subst(&mut self, subst: &Subst) -> Subst {
        let interner = self.cx.interner;
        let resolved = subst.map(|t| self.table.resolve(interner, t));
        resolved.map(|t| {
            let t = self.normalize(t);
            if interner.has_infer(t) {
                interner.error()
            } else {
                t
            }
        })
    }

    fn finalize_target(&mut self, target: crate::CallTarget) -> crate::CallTarget {
        use crate::CallTarget;
        match target {
            CallTarget::Fn { def, subst } => CallTarget::Fn {
                def,
                subst: self.finalize_subst(&subst),
            },
            CallTarget::Trait {
                method,
                self_ty,
                subst,
            } => {
                let self_ty = self.structurally(self_ty);
                CallTarget::Trait {
                    method,
                    self_ty,
                    subst: self.finalize_subst(&subst),
                }
            }
            CallTarget::Struct { def, subst } => CallTarget::Struct {
                def,
                subst: self.finalize_subst(&subst),
            },
            CallTarget::Variant { def, subst } => CallTarget::Variant {
                def,
                subst: self.finalize_subst(&subst),
            },
            CallTarget::Value => CallTarget::Value,
        }
    }

    fn finalize_info(&mut self, info: ExprInfo) -> ExprInfo {
        match info {
            ExprInfo::FnValue { def, subst } => ExprInfo::FnValue {
                def,
                subst: self.finalize_subst(&subst),
            },
            ExprInfo::Call {
                target,
                receiver,
                args,
            } => ExprInfo::Call {
                target: self.finalize_target(target),
                receiver,
                args,
            },
            ExprInfo::Try { result, convert } => ExprInfo::Try {
                result,
                convert: convert.map(|t| self.finalize_target(t)),
            },
            other => other,
        }
    }

    fn check_operators(&mut self) {
        let checks = std::mem::take(&mut self.operator_checks);
        for check in checks {
            let ty = self.structurally(check.ty);
            let ok = match self.cx.interner.kind(ty) {
                TyKind::Error | TyKind::Never => true,
                TyKind::Int(i) => match check.allowed {
                    Allowed::Signed => i.is_signed(),
                    _ => true,
                },
                TyKind::Float(_) => matches!(check.allowed, Allowed::Numeric | Allowed::Signed),
                TyKind::Bool => matches!(check.allowed, Allowed::IntegerOrBool),
                TyKind::Char => matches!(check.allowed, Allowed::IntegerOrChar),
                _ => false,
            };
            if !ok {
                let text = self.show(ty);
                self.error(
                    Diagnostic::error(
                        codes::UNSUPPORTED_OPERATOR,
                        self.span(check.expr),
                        format!("`{}` cannot be applied to `{text}`", check.op),
                    )
                    .with_label(format!("`{text}` does not support `{}`", check.op)),
                );
            }
        }
    }

    fn check_obligations(&mut self) {
        let obligations = std::mem::take(&mut self.obligations);
        let env = std::mem::take(&mut self.env);
        for obligation in obligations {
            let ty = self.resolve(obligation.pred.ty);
            let trait_ref = TraitRef {
                trait_: obligation.pred.trait_ref.trait_,
                args: obligation
                    .pred
                    .trait_ref
                    .args
                    .iter()
                    .map(|&a| self.resolve(a))
                    .collect(),
            };
            let result = self.solver().select(ty, &trait_ref, &env);
            if result == Err(SelectError::NoImpl) {
                let ty_text = self.show(ty);
                let trait_name = self.cx.hir.def(trait_ref.trait_).name.to_string();
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::UNSATISFIED_BOUND,
                        obligation.span,
                        format!("`{ty_text}` does not implement `{trait_name}`"),
                    )
                    .with_label(format!("`{trait_name}` is required here")),
                );
            }
        }
        self.env = env;
    }

    fn check_literals(&mut self) {
        let literals = std::mem::take(&mut self.int_literals);
        for (expr, value, negative) in literals {
            let ty =
                self.resolve(self.expr_tys[expr.index()].unwrap_or_else(|| self.cx.interner.int()));
            let fits = match self.cx.interner.kind(ty) {
                TyKind::Int(int) => fits(int, value, negative),
                TyKind::Float(_) => {
                    self.error(
                        Diagnostic::error(
                            codes::MISMATCHED_TYPES,
                            self.span(expr),
                            "an integer literal cannot have a float type",
                        )
                        .with_label("write a float literal such as `1.0`"),
                    );
                    true
                }
                _ => true,
            };
            if !fits {
                let text = self.show(ty);
                self.error(
                    Diagnostic::error(
                        codes::LITERAL_OUT_OF_RANGE,
                        self.span(expr),
                        format!("literal out of range for `{text}`"),
                    )
                    .with_label(format!("does not fit in `{text}`")),
                );
            }
        }
    }

    fn closure_kind_of_frame(frame: &ClosureFrame) -> FnKind {
        if frame.mutates_capture {
            FnKind::FnMut
        } else {
            FnKind::Fn
        }
    }

    fn def_kind(&self, def: DefId) -> &'a DefKind {
        &self.cx.hir.def(def).kind
    }
}

fn fits(int: IntTy, value: u128, negative: bool) -> bool {
    if negative {
        int.is_signed() && value <= int.min().unsigned_abs()
    } else {
        value <= int.max()
    }
}
