//! Checks on items: implementations match their traits (spec §3.5) and derives are valid
//! (spec §6.7).

use crate::items::{ImplKind, Items, TraitRef};
use crate::traits::{SelectError, Solver};
use crate::{Ctx, display::ty_to_string};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{DefKind, Name};
use loom_types::{InferTable, Subst, TyKind};

pub(crate) fn check_items(cx: &Ctx<'_>, diagnostics: &mut Vec<Diagnostic>) {
    for info in &cx.items.impls {
        match info.kind {
            ImplKind::Item(impl_def) => check_impl(cx, impl_def, info, diagnostics),
            ImplKind::Derived(type_def) => check_derive(cx, type_def, info, diagnostics),
        }
    }
}

fn check_impl(
    cx: &Ctx<'_>,
    impl_def: loom_hir::DefId,
    info: &crate::ImplInfo,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let hir = cx.hir;
    let trait_ = info.trait_ref.trait_;
    let Some(trait_info) = cx.items.traits.get(&trait_) else {
        return;
    };
    let trait_name = hir.def(trait_).name.clone();
    let DefKind::Impl(impl_data) = &hir.def(impl_def).kind else {
        return;
    };
    let mut missing: Vec<String> = Vec::new();
    for (name, &method) in &trait_info.methods {
        let provided = matches!(&hir.def(method).kind, DefKind::Fn(f) if f.is_default);
        if !provided && !info.methods.contains_key(name) {
            missing.push(format!("`{name}`"));
        }
    }
    for (name, &assoc) in &trait_info.assoc_types {
        if !info.assoc_types.contains_key(&assoc) {
            missing.push(format!("`type {name}`"));
        }
    }
    missing.sort();
    if !missing.is_empty() {
        diagnostics.push(
            Diagnostic::error(
                codes::MISSING_IMPL_ITEM,
                hir.def(impl_def).span,
                format!(
                    "missing {} in the implementation of `{trait_name}`",
                    missing.join(", ")
                ),
            )
            .with_label(format!("`{trait_name}` requires {}", missing.join(", "))),
        );
    }
    for &member in &impl_data.members {
        let m = hir.def(member);
        let known = match m.kind {
            DefKind::Fn(_) => trait_info.methods.contains_key(&m.name),
            DefKind::TypeAlias(_) => trait_info.assoc_types.contains_key(&m.name),
            DefKind::Const(_) => trait_info.consts.contains_key(&m.name),
            _ => true,
        };
        if !known {
            diagnostics.push(
                Diagnostic::error(
                    codes::NOT_A_TRAIT_MEMBER,
                    m.span,
                    format!("`{}` is not a member of trait `{trait_name}`", m.name),
                )
                .with_label("not declared in the trait")
                .with_help("inherent methods belong in the type's own body"),
            );
        } else if let DefKind::Fn(_) = m.kind
            && let Some(&decl) = trait_info.methods.get(&m.name)
        {
            check_method_signature(cx, info, member, decl, &trait_name, &m.name, diagnostics);
        }
    }
}

fn check_method_signature(
    cx: &Ctx<'_>,
    info: &crate::ImplInfo,
    member: loom_hir::DefId,
    decl: loom_hir::DefId,
    trait_name: &Name,
    name: &Name,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (Some(ours), Some(theirs)) = (cx.items.fns.get(&member), cx.items.fns.get(&decl)) else {
        return;
    };
    let interner = cx.interner;
    let mut subst = Subst::new();
    subst.insert(Items::self_param(info.trait_ref.trait_), info.self_ty);
    for (&p, &a) in cx
        .items
        .generics_of(info.trait_ref.trait_)
        .own
        .iter()
        .zip(&info.trait_ref.args)
    {
        subst.insert(p, a);
    }
    // The method's own generic parameters correspond by position.
    let decl_own = cx.items.generics_of(decl).own;
    let member_own = cx.items.generics_of(member).own;
    for (&d, &m) in decl_own.iter().zip(&member_own) {
        subst.insert(d, interner.intern(TyKind::Param(m)));
    }
    let mut table = InferTable::new();
    let env = Solver {
        hir: cx.hir,
        items: cx.items,
        interner,
        table: &mut table,
    }
    .elaborate(&cx.items.generics_of(member).predicates);
    let mut solver = Solver {
        hir: cx.hir,
        items: cx.items,
        interner,
        table: &mut table,
    };
    let mut same = |a, b| {
        let a = solver.normalize(interner.subst(a, &subst), &env);
        let b = solver.normalize(b, &env);
        a == b || interner.is_error(a) || interner.is_error(b)
    };
    let mut problem = None;
    if ours.receiver != theirs.receiver {
        problem = Some("the receiver mode differs from the trait's".to_owned());
    } else if ours.params.len() != theirs.params.len() {
        problem = Some(format!(
            "the trait's method takes {} parameters, this one takes {}",
            theirs.params.len(),
            ours.params.len()
        ));
    } else if decl_own.len() != member_own.len() {
        problem = Some("the number of generic parameters differs from the trait's".to_owned());
    } else {
        for (i, (o, t)) in ours.params.iter().zip(&theirs.params).enumerate() {
            if o.0 != t.0 {
                problem = Some(format!(
                    "parameter {} has a different mode than in the trait",
                    i + 1
                ));
                break;
            }
            if !same(t.2, o.2) {
                problem = Some(format!(
                    "parameter `{}` has type `{}`, but the trait expects `{}`",
                    o.1,
                    ty_to_string(cx.hir, cx.items, interner, o.2),
                    ty_to_string(cx.hir, cx.items, interner, interner.subst(t.2, &subst))
                ));
                break;
            }
        }
        if problem.is_none() && !same(theirs.ret, ours.ret) {
            problem = Some(format!(
                "the return type is `{}`, but the trait expects `{}`",
                ty_to_string(cx.hir, cx.items, interner, ours.ret),
                ty_to_string(
                    cx.hir,
                    cx.items,
                    interner,
                    interner.subst(theirs.ret, &subst)
                )
            ));
        }
    }
    if let Some(problem) = problem {
        diagnostics.push(
            Diagnostic::error(
                codes::IMPL_SIGNATURE_MISMATCH,
                cx.hir.def(member).span,
                format!("method `{name}` does not match its declaration in `{trait_name}`"),
            )
            .with_label(problem)
            .with_secondary(cx.hir.def(decl).span, "declared here"),
        );
    }
}

fn check_derive(
    cx: &Ctx<'_>,
    type_def: loom_hir::DefId,
    info: &crate::ImplInfo,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let hir = cx.hir;
    let interner = cx.interner;
    let trait_ = info.trait_ref.trait_;
    let lang = hir.def(trait_).lang().unwrap_or("");
    if !matches!(lang, "Copy" | "Clone" | "Eq" | "Ord" | "Hash" | "Default") {
        return;
    }
    if lang == "Default" && matches!(hir.def(type_def).kind, DefKind::Enum(_)) {
        diagnostics.push(
            Diagnostic::error(
                codes::INVALID_DERIVE,
                hir.def(type_def).span,
                "`Default` can only be derived for structs",
            )
            .with_label("an enum has no single default variant"),
        );
        return;
    }
    let mut field_tys = Vec::new();
    match &hir.def(type_def).kind {
        DefKind::Struct(_) => {
            field_tys.extend(
                cx.items
                    .fields
                    .get(&type_def)
                    .into_iter()
                    .flatten()
                    .map(|f| (f.name.to_string(), f.ty)),
            );
        }
        DefKind::Enum(data) => {
            for &v in &data.variants {
                match cx.items.variants.get(&v) {
                    Some(crate::VariantTy::Positional(types)) => {
                        field_tys.extend(types.iter().map(|&t| (hir.def(v).name.to_string(), t)));
                    }
                    Some(crate::VariantTy::Named(fields)) => {
                        field_tys.extend(
                            fields
                                .iter()
                                .map(|f| (format!("{}.{}", hir.def(v).name, f.name), f.ty)),
                        );
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    let mut table = InferTable::new();
    let mut solver = Solver {
        hir,
        items: cx.items,
        interner,
        table: &mut table,
    };
    let env = solver.elaborate(&info.predicates);
    let trait_ref = TraitRef {
        trait_,
        args: Vec::new(),
    };
    for (field, ty) in field_tys {
        // A recursive type implements the trait if its other fields do.
        if matches!(interner.kind(ty), TyKind::Adt(d, _) if d == type_def) {
            continue;
        }
        if solver.select(ty, &trait_ref, &env) == Err(SelectError::NoImpl) {
            let text = ty_to_string(hir, cx.items, interner, ty);
            diagnostics.push(
                Diagnostic::error(
                    codes::INVALID_DERIVE,
                    hir.def(type_def).span,
                    format!("cannot derive `{}`: field `{field}` has type `{text}`, which does not implement it", hir.def(trait_).name),
                )
                .with_label(format!("`{}` cannot be derived", hir.def(trait_).name)),
            );
            return;
        }
    }
}
