//! Lowering written types ([`TypeRef`]) to [`Ty`].

use crate::items::{Items, TraitRef};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{DefId, DefKind, Hir, Name, TypeRef, TypeRefKind};
use loom_resolve::{DefMap, PathError, Res, TyParamId};
use loom_span::Span;
use loom_types::{Interner, Subst, Ty, TyKind};

/// Lowers types written inside one item.
pub struct TyLowerer<'a> {
    hir: &'a Hir,
    def_map: &'a DefMap,
    items: &'a Items,
    interner: &'a Interner,
    /// The item whose generic parameters are in scope.
    scope: DefId,
    module: DefId,
}

impl<'a> TyLowerer<'a> {
    /// A lowerer for types written in `scope`.
    pub fn new(
        hir: &'a Hir,
        def_map: &'a DefMap,
        items: &'a Items,
        interner: &'a Interner,
        scope: DefId,
    ) -> Self {
        Self {
            hir,
            def_map,
            items,
            interner,
            scope,
            module: hir.def(scope).module,
        }
    }

    fn span(&self, ty: &TypeRef) -> Span {
        Span::new(self.file(), ty.range)
    }

    fn file(&self) -> loom_span::FileId {
        self.hir.def(self.scope).span.file
    }

    /// The type `Self` stands for in the scope, if any.
    pub fn self_ty(&self) -> Option<Ty> {
        let mut current = Some(self.scope);
        while let Some(def) = current {
            match &self.hir.def(def).kind {
                DefKind::Struct(_) | DefKind::Enum(_) => {
                    return Some(self.items.identity_ty(self.interner, def));
                }
                DefKind::Trait(_) => {
                    return Some(self.interner.intern(TyKind::Param(Items::self_param(def))));
                }
                DefKind::Impl(impl_data) => {
                    let lowerer =
                        TyLowerer::new(self.hir, self.def_map, self.items, self.interner, def);
                    return Some(lowerer.lower(&impl_data.self_ty, &mut Vec::new()));
                }
                DefKind::Module(_) => return None,
                _ => current = self.hir.def(def).parent,
            }
        }
        None
    }

    /// Lowers a type in a return position, where `ref T` is allowed (spec §5.6).
    pub fn lower_return(&self, ty: &TypeRef, diagnostics: &mut Vec<Diagnostic>) -> Ty {
        self.lower_inner(ty, true, diagnostics)
    }

    /// Lowers a type.
    pub fn lower(&self, ty: &TypeRef, diagnostics: &mut Vec<Diagnostic>) -> Ty {
        self.lower_inner(ty, false, diagnostics)
    }

    fn lower_inner(&self, ty: &TypeRef, in_return: bool, diagnostics: &mut Vec<Diagnostic>) -> Ty {
        let interner = self.interner;
        match &ty.kind {
            TypeRefKind::Error => interner.error(),
            TypeRefKind::SelfType => self.self_ty().unwrap_or_else(|| {
                diagnostics.push(
                    Diagnostic::error(
                        codes::SELF_OUTSIDE_METHOD,
                        self.span(ty),
                        "`Self` is only available inside a type, trait or `impl`",
                    )
                    .with_label("not inside a type"),
                );
                interner.error()
            }),
            TypeRefKind::Tuple(elements) => {
                if elements.is_empty() {
                    interner.unit()
                } else {
                    let elements = elements
                        .iter()
                        .map(|e| self.lower_inner(e, in_return, diagnostics))
                        .collect();
                    interner.intern(TyKind::Tuple(elements))
                }
            }
            TypeRefKind::Fn { kind, params, ret } => {
                let params = params.iter().map(|p| self.lower(p, diagnostics)).collect();
                let ret = ret.as_ref().map_or_else(
                    || interner.unit(),
                    |r| self.lower_inner(r, true, diagnostics),
                );
                interner.intern(TyKind::Callable {
                    kind: *kind,
                    params,
                    ret,
                })
            }
            TypeRefKind::Dyn(bounds) => {
                let Some(first) = bounds.first() else {
                    return interner.error();
                };
                match self.lower_trait_ref(first, diagnostics) {
                    Some(trait_ref) => {
                        interner.intern(TyKind::Dyn(trait_ref.trait_, trait_ref.args))
                    }
                    None => interner.error(),
                }
            }
            TypeRefKind::Ref { inner, .. } => {
                if !in_return {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::REF_OUTSIDE_RETURN,
                            self.span(ty),
                            "`ref` types are only allowed in a function's return type",
                        )
                        .with_label("not a return type")
                        .with_help(
                            "parameters are borrowed by default; write the type without `ref`",
                        ),
                    );
                }
                // Phase 2 checks `ref T` as `T` (ADR 0019 §3 rule 2).
                self.lower_inner(inner, in_return, diagnostics)
            }
            TypeRefKind::Path(path) => self.lower_path(ty, path, in_return, diagnostics),
        }
    }

    fn lower_path(
        &self,
        ty: &TypeRef,
        path: &loom_hir::Path,
        in_return: bool,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Ty {
        let interner = self.interner;
        let (res, consumed) =
            match self
                .def_map
                .resolve_type_path(self.hir, Some(self.scope), self.module, path)
            {
                Ok(r) => r,
                Err(PathError::NotFound(i) | PathError::Private(i, _)) => {
                    diagnostics.push(
                        Diagnostic::error(
                            codes::UNRESOLVED_NAME,
                            Span::new(self.file(), path.segments[i].range),
                            format!("cannot find type `{}` in this scope", path.segments[i].name),
                        )
                        .with_label("not found"),
                    );
                    return interner.error();
                }
            };
        let segment = &path.segments[consumed - 1];
        let args: Vec<Ty> = segment
            .args
            .iter()
            .map(|a| self.lower_inner(a, in_return, diagnostics))
            .collect();
        let mut base = match res {
            Res::SelfType => self.self_ty().unwrap_or_else(|| interner.error()),
            Res::TyParam(param) => interner.intern(TyKind::Param(param)),
            Res::Def(def) => match self.def_ty(def, args, ty, diagnostics) {
                Some(t) => t,
                None => return interner.error(),
            },
            Res::Local(_) | Res::SelfValue | Res::Err => interner.error(),
        };
        for segment in &path.segments[consumed..] {
            base = self.projection(
                base,
                &segment.name,
                Span::new(self.file(), segment.range),
                diagnostics,
            );
        }
        base
    }

    /// The type named by an item with generic arguments `args`.
    fn def_ty(
        &self,
        def: DefId,
        args: Vec<Ty>,
        ty: &TypeRef,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<Ty> {
        let interner = self.interner;
        let data = self.hir.def(def);
        match &data.kind {
            DefKind::Struct(_) | DefKind::Enum(_) => {
                if let Some(prim) = self.items.lang.prims.get(&def) {
                    return Some(interner.intern(prim.clone()));
                }
                let expected = self.hir.own_generics(def).map_or(0, |g| g.params.len());
                let args = self.check_arg_count(def, args, expected, ty, diagnostics);
                Some(interner.intern(TyKind::Adt(def, args)))
            }
            DefKind::TypeAlias(_) => {
                let target = self
                    .items
                    .aliases
                    .get(&def)
                    .copied()
                    .unwrap_or_else(|| interner.error());
                let generics = self.items.generics_of(def);
                let args = self.check_arg_count(def, args, generics.own.len(), ty, diagnostics);
                let mut subst = Subst::new();
                for (&p, &a) in generics.own.iter().zip(&args) {
                    subst.insert(p, a);
                }
                Some(interner.subst(target, &subst))
            }
            DefKind::Trait(_) => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::WRONG_NAME_KIND,
                        self.span(ty),
                        format!("expected a type, found trait `{}`", data.name),
                    )
                    .with_label("a trait is not a type")
                    .with_help(format!(
                        "use `dyn {}` for a value of any type implementing it",
                        data.name
                    )),
                );
                None
            }
            other => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::WRONG_NAME_KIND,
                        self.span(ty),
                        format!(
                            "expected a type, found {} `{}`",
                            other.describe(),
                            data.name
                        ),
                    )
                    .with_label("not a type"),
                );
                None
            }
        }
    }

    fn check_arg_count(
        &self,
        def: DefId,
        args: Vec<Ty>,
        expected: usize,
        ty: &TypeRef,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Vec<Ty> {
        if args.len() == expected {
            return args;
        }
        diagnostics.push(
            Diagnostic::error(
                codes::WRONG_GENERIC_COUNT,
                self.span(ty),
                format!(
                    "`{}` takes {expected} generic argument{}, but {} were given",
                    self.hir.def(def).name,
                    if expected == 1 { "" } else { "s" },
                    args.len()
                ),
            )
            .with_label(format!("expected {expected}")),
        );
        (0..expected)
            .map(|i| {
                args.get(i)
                    .copied()
                    .unwrap_or_else(|| self.interner.error())
            })
            .collect()
    }

    /// `base.Name`: an associated type of a trait that `base` is known to implement.
    pub fn projection(
        &self,
        base: Ty,
        name: &Name,
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Ty {
        let interner = self.interner;
        if interner.is_error(base) {
            return base;
        }
        for trait_ref in self.candidate_traits(base) {
            if let Some((owner, assoc)) = self.find_assoc(trait_ref.trait_, name) {
                let args = if owner == trait_ref.trait_ {
                    let mut args = vec![base];
                    args.extend(trait_ref.args.iter().copied());
                    args
                } else {
                    vec![base]
                };
                return interner.intern(TyKind::Projection {
                    trait_: owner,
                    args,
                    assoc,
                });
            }
        }
        diagnostics.push(
            Diagnostic::error(
                codes::UNKNOWN_ASSOCIATED_ITEM,
                span,
                format!("no associated type `{name}` is known here"),
            )
            .with_label("not found")
            .with_help("add a bound naming the trait that declares it"),
        );
        interner.error()
    }

    /// The trait (or supertrait) of `trait_` declaring associated type `name`.
    fn find_assoc(&self, trait_: DefId, name: &Name) -> Option<(DefId, DefId)> {
        let mut stack = vec![trait_];
        let mut seen = Vec::new();
        while let Some(t) = stack.pop() {
            if seen.contains(&t) {
                continue;
            }
            seen.push(t);
            if let DefKind::Trait(data) = &self.hir.def(t).kind {
                for &member in &data.members {
                    let m = self.hir.def(member);
                    if matches!(m.kind, DefKind::AssocType(_)) && m.name == *name {
                        return Some((t, member));
                    }
                }
                for sup in &data.supertraits {
                    if let Some(Ok(def)) = self.def_map.resolve_trait_ref(
                        self.hir,
                        Some(t),
                        self.hir.def(t).module,
                        sup,
                    ) {
                        stack.push(def);
                    }
                }
            }
        }
        None
    }

    /// Traits `ty` is known to implement from the bounds in scope, read directly from
    /// HIR so this works while predicates are still being collected.
    fn candidate_traits(&self, ty: Ty) -> Vec<TraitRef> {
        let mut out = Vec::new();
        match self.interner.kind(ty) {
            TyKind::Param(param) if param.index == loom_types::SELF_INDEX => {
                out.push(TraitRef {
                    trait_: param.owner,
                    args: Vec::new(),
                });
            }
            TyKind::Param(param) => self.param_bounds(param, &mut out),
            _ => {}
        }
        // Where clauses of the scope and its parents.
        let mut current = Some(self.scope);
        while let Some(def) = current {
            if let Some(generics) = self.hir.own_generics(def) {
                let lowerer =
                    TyLowerer::new(self.hir, self.def_map, self.items, self.interner, def);
                for (pred_ty, bounds) in &generics.where_preds {
                    if matches!(pred_ty.kind, TypeRefKind::Path(ref p) if p.segments.len() > 1) {
                        continue;
                    }
                    if lowerer.lower(pred_ty, &mut Vec::new()) == ty {
                        out.extend(
                            bounds
                                .iter()
                                .filter_map(|b| lowerer.lower_trait_ref(b, &mut Vec::new())),
                        );
                    }
                }
            }
            current = self.hir.generics_parent(def);
        }
        if let TyKind::Projection { trait_, assoc, .. } = self.interner.kind(ty)
            && let Some(bounds) = self
                .items
                .traits
                .get(&trait_)
                .and_then(|t| t.assoc_bounds.get(&assoc))
        {
            out.extend(bounds.iter().cloned());
        }
        if out.is_empty() {
            // A concrete type: any trait declaring an associated type of that name.
            for (&trait_, info) in &self.items.traits {
                if !info.assoc_types.is_empty() {
                    out.push(TraitRef {
                        trait_,
                        args: Vec::new(),
                    });
                }
            }
            out.sort_by_key(|t| t.trait_);
        }
        out
    }

    fn param_bounds(&self, param: TyParamId, out: &mut Vec<TraitRef>) {
        let Some(generics) = self.hir.own_generics(param.owner) else {
            return;
        };
        let Some(p) = generics.params.get(param.index as usize) else {
            return;
        };
        let lowerer = TyLowerer::new(
            self.hir,
            self.def_map,
            self.items,
            self.interner,
            param.owner,
        );
        for bound in &p.bounds {
            if let Some(trait_ref) = lowerer.lower_trait_ref(bound, &mut Vec::new()) {
                out.push(trait_ref);
            }
        }
    }

    /// Lowers a trait path such as `Ord` or `From[String]`.
    pub fn lower_trait_ref(
        &self,
        ty: &TypeRef,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<TraitRef> {
        let TypeRefKind::Path(path) = &ty.kind else {
            diagnostics.push(
                Diagnostic::error(codes::EXPECTED_TRAIT, self.span(ty), "expected a trait")
                    .with_label("not a trait"),
            );
            return None;
        };
        match self
            .def_map
            .resolve_trait_ref(self.hir, Some(self.scope), self.module, ty)
        {
            Some(Ok(def)) if matches!(self.hir.def(def).kind, DefKind::Trait(_)) => {
                let args = path
                    .segments
                    .last()
                    .map(|s| s.args.iter().map(|a| self.lower(a, diagnostics)).collect())
                    .unwrap_or_default();
                Some(TraitRef { trait_: def, args })
            }
            Some(Ok(def)) => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::EXPECTED_TRAIT,
                        self.span(ty),
                        format!(
                            "expected a trait, found {} `{}`",
                            self.hir.def(def).kind.describe(),
                            path.text()
                        ),
                    )
                    .with_label("not a trait"),
                );
                None
            }
            Some(Err(PathError::NotFound(i) | PathError::Private(i, _))) => {
                diagnostics.push(
                    Diagnostic::error(
                        codes::UNRESOLVED_NAME,
                        Span::new(self.file(), path.segments[i].range),
                        format!(
                            "cannot find trait `{}` in this scope",
                            path.segments[i].name
                        ),
                    )
                    .with_label("not found"),
                );
                None
            }
            None => None,
        }
    }
}
