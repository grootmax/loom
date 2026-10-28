//! Patterns (spec §9.1).

use super::InferCtxt;
use crate::{PatInfo, VariantTy};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{DefId, DefKind, Literal, Name, Pat, PatId, Path};
use loom_resolve::Res;
use loom_span::TextRange;
use loom_types::{FloatTy, Subst, Ty, TyKind, VarKind};

impl InferCtxt<'_> {
    /// Checks `pat` against the type of the matched value.
    #[allow(clippy::too_many_lines, reason = "one case per language construct")]
    #[allow(clippy::similar_names, reason = "names mirror the spec terms")]
    pub(super) fn check_pat(&mut self, pat: PatId, expected: Ty) {
        let interner = self.cx.interner;
        // Matching through a borrow: the pattern sees the borrowed value, and its
        // bindings borrow parts of it (spec §8.3 rule 6).
        if !matches!(
            self.body.pat(pat),
            Pat::Bind(_) | Pat::Wild | Pat::Missing | Pat::Rest
        ) {
            let resolved = self.structurally(expected);
            if let TyKind::Ref { mutable, inner } = interner.kind(resolved) {
                let saved = self.binding_ref.replace(mutable);
                self.check_pat(pat, inner);
                self.binding_ref = saved;
                return;
            }
        }
        self.pat_tys[pat.index()] = Some(expected);
        match self.body.pat(pat) {
            Pat::Missing | Pat::Wild | Pat::Rest => {}
            &Pat::Bind(binding) => {
                let already_ref = matches!(
                    interner.kind(self.structurally(expected)),
                    TyKind::Ref { .. }
                );
                let ty = match self.binding_ref {
                    Some(mutable) if !already_ref => interner.intern(TyKind::Ref {
                        mutable,
                        inner: expected,
                    }),
                    _ => expected,
                };
                self.declare_binding(binding, ty);
            }
            Pat::Literal { lit, negative } => {
                let _ = negative;
                let ty = self.pat_literal_ty(lit);
                self.unify_pat(pat, ty, expected);
            }
            Pat::Range { lo, hi, .. } => {
                let ty = self.pat_literal_ty(&lo.0);
                let hi_ty = self.pat_literal_ty(&hi.0);
                let _ = self.table.unify(interner, ty, hi_ty);
                self.unify_pat(pat, ty, expected);
                let resolved = self.structurally(ty);
                if !matches!(
                    interner.kind(resolved),
                    TyKind::Int(_) | TyKind::Char | TyKind::Infer(_, VarKind::Int) | TyKind::Error
                ) {
                    self.error(
                        Diagnostic::error(
                            codes::INVALID_RANGE_PATTERN,
                            self.body.pat_span(pat),
                            "range patterns need integer or character bounds",
                        )
                        .with_label("invalid range"),
                    );
                }
            }
            Pat::Tuple(elements) => {
                let elements = elements.clone();
                let expected_resolved = self.structurally(expected);
                let rest = elements
                    .iter()
                    .position(|&e| matches!(self.body.pat(e), Pat::Rest));
                let fixed: Vec<PatId> = elements
                    .iter()
                    .copied()
                    .filter(|&e| !matches!(self.body.pat(e), Pat::Rest))
                    .collect();
                let element_tys = match interner.kind(expected_resolved) {
                    TyKind::Tuple(tys)
                        if tys.len() == elements.len()
                            || (rest.is_some() && tys.len() >= fixed.len()) =>
                    {
                        tys
                    }
                    TyKind::Infer(..) if rest.is_none() => {
                        let tys: Vec<Ty> = elements.iter().map(|_| self.new_var()).collect();
                        let tuple = interner.intern(TyKind::Tuple(tys.clone()));
                        let _ = self.table.unify(interner, tuple, expected_resolved);
                        tys
                    }
                    TyKind::Error => vec![interner.error(); elements.len()],
                    _ => {
                        let text = self.show(expected);
                        self.error(
                            Diagnostic::error(
                                codes::PATTERN_TYPE_MISMATCH,
                                self.body.pat_span(pat),
                                format!(
                                    "a tuple pattern with {} elements cannot match `{text}`",
                                    fixed.len()
                                ),
                            )
                            .with_label("wrong pattern shape"),
                        );
                        vec![interner.error(); elements.len()]
                    }
                };
                match rest {
                    None => {
                        for (i, &e) in elements.iter().enumerate() {
                            let t = element_tys
                                .get(i)
                                .copied()
                                .unwrap_or_else(|| interner.error());
                            self.check_pat(e, t);
                        }
                    }
                    Some(r) => {
                        let after = elements.len() - r - 1;
                        for (i, &e) in elements[..r].iter().enumerate() {
                            self.check_pat(
                                e,
                                element_tys
                                    .get(i)
                                    .copied()
                                    .unwrap_or_else(|| interner.error()),
                            );
                        }
                        for (i, &e) in elements[r + 1..].iter().enumerate() {
                            let index = element_tys.len().saturating_sub(after) + i;
                            self.check_pat(
                                e,
                                element_tys
                                    .get(index)
                                    .copied()
                                    .unwrap_or_else(|| interner.error()),
                            );
                        }
                    }
                }
            }
            Pat::Path(path) => {
                let path = path.clone();
                self.path_pat(pat, &path, expected, PayloadPat::None);
            }
            Pat::TupleStruct { path, pats, rest } => {
                let (path, pats, rest) = (path.clone(), pats.clone(), *rest);
                self.path_pat(pat, &path, expected, PayloadPat::Positional(&pats, rest));
            }
            Pat::Struct { path, fields, rest } => {
                let (path, fields, rest) = (path.clone(), fields.clone(), *rest);
                self.path_pat(pat, &path, expected, PayloadPat::Named(&fields, rest));
            }
            Pat::Or(alternatives) => {
                for &alternative in &alternatives.clone() {
                    self.check_pat(alternative, expected);
                }
            }
        }
    }

    fn pat_literal_ty(&mut self, lit: &Literal) -> Ty {
        let interner = self.cx.interner;
        match lit {
            Literal::Int(_) => self.table.new_var(interner, VarKind::Int),
            Literal::Float(_) => {
                let _ = FloatTy::F64;
                self.table.new_var(interner, VarKind::Float)
            }
            Literal::Char(_) => interner.intern(TyKind::Char),
            Literal::String(_) => interner.string(),
            Literal::Bytes(_) => interner.intern(TyKind::Bytes),
            Literal::Bool(_) => interner.bool(),
        }
    }

    fn unify_pat(&mut self, pat: PatId, actual: Ty, expected: Ty) {
        if self
            .table
            .unify(self.cx.interner, actual, expected)
            .is_err()
        {
            let (e, a) = (self.show(expected), self.show(actual));
            self.error(
                Diagnostic::error(
                    codes::MISMATCHED_TYPES,
                    self.body.pat_span(pat),
                    format!("mismatched types: this pattern matches `{a}`, but the value is `{e}`"),
                )
                .with_label(format!("expected `{e}`")),
            );
        }
    }

    fn path_pat(&mut self, pat: PatId, path: &Path, expected: Ty, payload: PayloadPat<'_>) {
        let interner = self.cx.interner;
        let target = if self.res.unresolved_pats.contains(&pat) {
            let name = path
                .segments
                .first()
                .map_or_else(|| Name::new("_"), |s| s.name.clone());
            if let Some((variant, _, _)) = self.variant_from_expected(&name, Some(expected)) {
                Some(variant)
            } else {
                let e = self.structurally(expected);
                let message = match interner.kind(e) {
                    TyKind::Adt(def, _) if matches!(self.def_kind(def), DefKind::Enum(_)) => {
                        format!(
                            "no variant `{name}` in enum `{}`",
                            self.cx.hir.def(def).name
                        )
                    }
                    TyKind::Error => {
                        self.bind_payload_errors(payload);
                        return;
                    }
                    _ => format!(
                        "cannot find `{name}`: the matched value is not an enum with such a variant"
                    ),
                };
                self.error(
                    Diagnostic::error(codes::UNKNOWN_VARIANT, self.body.pat_span(pat), message)
                        .with_label("unknown variant"),
                );
                self.bind_payload_errors(payload);
                return;
            }
        } else {
            match self.res.pats.get(&pat) {
                Some(Res::Def(def)) => Some(*def),
                _ => None,
            }
        };
        let Some(def) = target else {
            self.bind_payload_errors(payload);
            return;
        };
        match self.def_kind(def) {
            DefKind::Variant(_) => {
                let enum_def = self.cx.hir.def(def).parent.unwrap_or(def);
                let (enum_ty, subst) = self.instantiate_adt(enum_def, Some(expected));
                self.unify_pat(pat, enum_ty, expected);
                self.pats.insert(pat, PatInfo::Variant(def));
                let fields = self
                    .cx
                    .items
                    .variants
                    .get(&def)
                    .cloned()
                    .unwrap_or(VariantTy::Unit);
                self.payload(pat, def, &fields, &subst, payload);
            }
            DefKind::Struct(_) => {
                let (ty, subst) = self.instantiate_adt(def, Some(expected));
                self.unify_pat(pat, ty, expected);
                self.pats.insert(pat, PatInfo::Struct(def));
                let fields =
                    VariantTy::Named(self.cx.items.fields.get(&def).cloned().unwrap_or_default());
                self.payload(pat, def, &fields, &subst, payload);
            }
            DefKind::Const(_) => {
                let ty = self
                    .cx
                    .items
                    .consts
                    .get(&def)
                    .copied()
                    .unwrap_or_else(|| interner.error());
                self.unify_pat(pat, ty, expected);
                self.pats.insert(pat, PatInfo::Const(def));
                self.require(ty, "Eq", Vec::new(), self.body.pat_span(pat));
                if !matches!(payload, PayloadPat::None) {
                    self.shape_error(pat, "a constant has no fields");
                }
            }
            other => {
                let what = format!("{} `{}`", other.describe(), self.cx.hir.def(def).name);
                self.error(
                    Diagnostic::error(
                        codes::WRONG_NAME_KIND,
                        self.body.pat_span(pat),
                        format!("expected a variant, struct or constant, found {what}"),
                    )
                    .with_label("cannot be matched"),
                );
                self.bind_payload_errors(payload);
            }
        }
    }

    fn shape_error(&mut self, pat: PatId, message: &str) {
        self.error(
            Diagnostic::error(
                codes::PATTERN_TYPE_MISMATCH,
                self.body.pat_span(pat),
                message.to_owned(),
            )
            .with_label("wrong pattern shape"),
        );
    }

    fn bind_payload_errors(&mut self, payload: PayloadPat<'_>) {
        let error = self.cx.interner.error();
        match payload {
            PayloadPat::None => {}
            PayloadPat::Positional(pats, _) => {
                for &p in pats {
                    self.check_pat(p, error);
                }
            }
            PayloadPat::Named(fields, _) => {
                for &(_, _, p) in fields {
                    self.check_pat(p, error);
                }
            }
        }
    }

    fn payload(
        &mut self,
        pat: PatId,
        def: DefId,
        fields: &VariantTy,
        subst: &Subst,
        payload: PayloadPat<'_>,
    ) {
        let interner = self.cx.interner;
        let name = self.cx.hir.def(def).name.clone();
        match (fields, payload) {
            (VariantTy::Unit, PayloadPat::None)
            | (VariantTy::Named(_) | VariantTy::Positional(_), PayloadPat::Positional([], true)) => {
            }
            (VariantTy::Positional(types), PayloadPat::Positional(pats, rest)) => {
                if pats.len() > types.len() || (!rest && pats.len() != types.len()) {
                    self.shape_error(
                        pat,
                        &format!(
                            "`{name}` has {} field{}, but the pattern has {}",
                            types.len(),
                            if types.len() == 1 { "" } else { "s" },
                            pats.len()
                        ),
                    );
                }
                for (i, &p) in pats.iter().enumerate() {
                    let t = types
                        .get(i)
                        .map_or_else(|| interner.error(), |&t| interner.subst(t, subst));
                    let t = self.normalize(t);
                    self.check_pat(p, t);
                }
            }
            (VariantTy::Named(named), PayloadPat::Named(fields, rest)) => {
                let mut seen = Vec::new();
                for &(ref field, range, p) in fields {
                    if let Some(f) = named.iter().find(|f| f.name == *field) {
                        let t = interner.subst(f.ty, subst);
                        let t = self.normalize(t);
                        self.check_pat(p, t);
                        seen.push(field.clone());
                    } else {
                        self.unknown_pat_field(range, &name, field);
                        self.check_pat(p, interner.error());
                    }
                }
                if !rest {
                    let missing: Vec<String> = named
                        .iter()
                        .filter(|f| !seen.contains(&f.name))
                        .map(|f| format!("`{}`", f.name))
                        .collect();
                    if !missing.is_empty() {
                        self.shape_error(
                            pat,
                            &format!(
                                "pattern does not mention field{} {}; add them or `..`",
                                if missing.len() == 1 { "" } else { "s" },
                                missing.join(", ")
                            ),
                        );
                    }
                }
            }
            (VariantTy::Unit, _) => self.shape_error(pat, &format!("`{name}` has no fields")),
            (_, PayloadPat::None) => self.shape_error(
                pat,
                &format!("`{name}` has fields; write `{name}(..)` to ignore them"),
            ),
            (VariantTy::Positional(_), PayloadPat::Named(..)) => {
                self.shape_error(pat, &format!("`{name}` has positional fields"));
            }
            (VariantTy::Named(_), PayloadPat::Positional(..)) => self.shape_error(
                pat,
                &format!("`{name}` has named fields; write `field:` patterns"),
            ),
        }
    }

    fn unknown_pat_field(&mut self, range: TextRange, owner: &Name, field: &Name) {
        self.error(
            Diagnostic::error(
                codes::UNKNOWN_FIELD,
                loom_span::Span::new(self.body.file, range),
                format!("`{owner}` has no field named `{field}`"),
            )
            .with_label("unknown field"),
        );
    }
}

#[derive(Clone, Copy)]
enum PayloadPat<'p> {
    None,
    Positional(&'p [PatId], bool),
    Named(&'p [(Name, TextRange, PatId)], bool),
}
