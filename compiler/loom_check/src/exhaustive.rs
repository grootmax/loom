//! Exhaustiveness and reachability of patterns (spec §9.2, §9.4), using the usefulness
//! algorithm of Maranget, "Warnings for pattern matching" (2007).
//!
//! Integers, characters, strings and floats are treated as having infinitely many
//! constructors, so only a wildcard or a binding covers them.

use crate::{Items, PatInfo, VariantTy};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{Body, DefId, DefKind, Expr, ExprId, Hir, Literal, Pat, PatId};
use loom_types::{Interner, Ty, TyKind};
use std::collections::HashMap;

/// What the checker needs to see of a typed body.
pub(crate) struct View<'a> {
    pub hir: &'a Hir,
    pub items: &'a Items,
    pub interner: &'a Interner,
    pub body: &'a Body,
    pub pat_tys: &'a [Ty],
    pub pats: &'a HashMap<PatId, PatInfo>,
}

#[derive(Clone, Debug, PartialEq)]
enum Ctor {
    Variant(DefId),
    Single,
    Bool(bool),
    /// A literal, compared by its text (opaque: never complete).
    Literal(String),
    /// A constant pattern (opaque).
    Const(DefId),
}

#[derive(Clone, Debug)]
struct DPat {
    ctor: Option<Ctor>,
    fields: Vec<DPat>,
    ty: Ty,
}

impl DPat {
    fn wild(ty: Ty) -> Self {
        Self {
            ctor: None,
            fields: Vec::new(),
            ty,
        }
    }
}

impl View<'_> {
    /// The field types of `ctor` for a value of type `ty`.
    fn ctor_fields(&self, ctor: &Ctor, ty: Ty) -> Vec<Ty> {
        let interner = self.interner;
        match (ctor, interner.kind(ty)) {
            (Ctor::Single, TyKind::Tuple(elements)) => elements,
            (Ctor::Single, TyKind::Adt(def, args)) => {
                let subst = self.items.adt_subst(def, &args);
                self.items
                    .fields
                    .get(&def)
                    .map(|fs| fs.iter().map(|f| interner.subst(f.ty, &subst)).collect())
                    .unwrap_or_default()
            }
            (Ctor::Variant(variant), TyKind::Adt(def, args)) => {
                let subst = self.items.adt_subst(def, &args);
                match self.items.variants.get(variant) {
                    Some(VariantTy::Positional(types)) => {
                        types.iter().map(|&t| interner.subst(t, &subst)).collect()
                    }
                    Some(VariantTy::Named(fields)) => fields
                        .iter()
                        .map(|f| interner.subst(f.ty, &subst))
                        .collect(),
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    /// Every constructor of `ty`, or `None` if there are infinitely many.
    fn all_ctors(&self, ty: Ty) -> Option<Vec<Ctor>> {
        match self.interner.kind(ty) {
            TyKind::Bool => Some(vec![Ctor::Bool(false), Ctor::Bool(true)]),
            TyKind::Unit | TyKind::Tuple(_) => Some(vec![Ctor::Single]),
            TyKind::Adt(def, _) => match &self.hir.def(def).kind {
                DefKind::Enum(data) => {
                    Some(data.variants.iter().map(|&v| Ctor::Variant(v)).collect())
                }
                DefKind::Struct(_)
                    if !self.items.lang.prims.contains_key(&def)
                        && self.hir.def(def).lang().is_none() =>
                {
                    Some(vec![Ctor::Single])
                }
                _ => None,
            },
            TyKind::Never => Some(Vec::new()),
            _ => None,
        }
    }

    #[allow(clippy::similar_names, reason = "names mirror the spec terms")]
    fn lower(&self, pat: PatId) -> Vec<DPat> {
        let ty = self.pat_tys[pat.index()];
        let one = |ctor: Option<Ctor>, fields: Vec<DPat>| vec![DPat { ctor, fields, ty }];
        match self.body.pat(pat) {
            Pat::Missing | Pat::Wild | Pat::Bind(_) | Pat::Rest => one(None, Vec::new()),
            Pat::Literal { lit, negative } => {
                let ctor = match lit {
                    Literal::Bool(b) => Ctor::Bool(*b),
                    other => {
                        Ctor::Literal(format!("{}{other:?}", if *negative { "-" } else { "" }))
                    }
                };
                one(Some(ctor), Vec::new())
            }
            Pat::Range { lo, hi, inclusive } => one(
                Some(Ctor::Literal(format!("{lo:?}..{inclusive}{hi:?}"))),
                Vec::new(),
            ),
            Pat::Tuple(elements) => {
                let types = match self.interner.kind(ty) {
                    TyKind::Tuple(ts) => ts,
                    _ => Vec::new(),
                };
                let rest = elements
                    .iter()
                    .position(|&e| matches!(self.body.pat(e), Pat::Rest));
                let mut fields: Vec<Vec<DPat>> = Vec::new();
                match rest {
                    None => {
                        for &e in elements {
                            fields.push(self.lower(e));
                        }
                    }
                    Some(r) => {
                        let after = elements.len() - r - 1;
                        for &e in &elements[..r] {
                            fields.push(self.lower(e));
                        }
                        let middle = types.len().saturating_sub(r + after);
                        for i in 0..middle {
                            fields.push(vec![DPat::wild(types.get(r + i).copied().unwrap_or(ty))]);
                        }
                        for &e in &elements[r + 1..] {
                            fields.push(self.lower(e));
                        }
                    }
                }
                product(fields)
                    .into_iter()
                    .map(|f| DPat {
                        ctor: Some(Ctor::Single),
                        fields: f,
                        ty,
                    })
                    .collect()
            }
            Pat::Path(_) | Pat::TupleStruct { .. } | Pat::Struct { .. } => {
                let ctor = match self.pats.get(&pat) {
                    Some(PatInfo::Variant(def)) => Ctor::Variant(*def),
                    Some(PatInfo::Struct(_)) => Ctor::Single,
                    Some(PatInfo::Const(def)) => return one(Some(Ctor::Const(*def)), Vec::new()),
                    None => return one(None, Vec::new()),
                };
                let field_tys = self.ctor_fields(&ctor, ty);
                let mut fields: Vec<Vec<DPat>> =
                    field_tys.iter().map(|&t| vec![DPat::wild(t)]).collect();
                match self.body.pat(pat) {
                    Pat::TupleStruct { pats, .. } => {
                        for (i, &p) in pats.iter().enumerate() {
                            if i < fields.len() {
                                fields[i] = self.lower(p);
                            }
                        }
                    }
                    Pat::Struct { fields: named, .. } => {
                        let names = self.field_names(&ctor, ty);
                        for (name, _, p) in named {
                            if let Some(i) = names.iter().position(|n| n == name) {
                                fields[i] = self.lower(*p);
                            }
                        }
                    }
                    _ => {}
                }
                product(fields)
                    .into_iter()
                    .map(|f| DPat {
                        ctor: Some(ctor.clone()),
                        fields: f,
                        ty,
                    })
                    .collect()
            }
            Pat::Or(alternatives) => alternatives.iter().flat_map(|&a| self.lower(a)).collect(),
        }
    }

    fn field_names(&self, ctor: &Ctor, ty: Ty) -> Vec<loom_hir::Name> {
        match (ctor, self.interner.kind(ty)) {
            (Ctor::Single, TyKind::Adt(def, _)) => self
                .items
                .fields
                .get(&def)
                .map(|fs| fs.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default(),
            (Ctor::Variant(v), _) => match self.items.variants.get(v) {
                Some(VariantTy::Named(fields)) => fields.iter().map(|f| f.name.clone()).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn specialize(&self, row: &[DPat], ctor: &Ctor) -> Option<Vec<DPat>> {
        let (head, rest) = row.split_first()?;
        let mut out = match &head.ctor {
            None => self
                .ctor_fields(ctor, head.ty)
                .into_iter()
                .map(DPat::wild)
                .collect(),
            Some(c) if c == ctor => head.fields.clone(),
            Some(_) => return None,
        };
        out.extend(rest.iter().cloned());
        Some(out)
    }

    fn useful(&self, matrix: &[Vec<DPat>], vector: &[DPat], depth: u32) -> bool {
        if depth > 64 {
            return true;
        }
        let Some((head, rest)) = vector.split_first() else {
            return matrix.is_empty();
        };
        if let Some(ctor) = &head.ctor {
            let specialized: Vec<Vec<DPat>> = matrix
                .iter()
                .filter_map(|r| self.specialize(r, ctor))
                .collect();
            let v = self.specialize(vector, ctor).unwrap_or_default();
            return self.useful(&specialized, &v, depth + 1);
        }
        match self.all_ctors(head.ty) {
            Some(ctors) if !ctors.is_empty() && self.column_complete(matrix, &ctors) => {
                ctors.iter().any(|ctor| {
                    let specialized: Vec<Vec<DPat>> = matrix
                        .iter()
                        .filter_map(|r| self.specialize(r, ctor))
                        .collect();
                    let v = self.specialize(vector, ctor).unwrap_or_default();
                    self.useful(&specialized, &v, depth + 1)
                })
            }
            _ => {
                let default: Vec<Vec<DPat>> = matrix
                    .iter()
                    .filter(|r| r.first().is_some_and(|h| h.ctor.is_none()))
                    .map(|r| r[1..].to_vec())
                    .collect();
                self.useful(&default, rest, depth + 1)
            }
        }
    }

    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    fn column_complete(&self, matrix: &[Vec<DPat>], ctors: &[Ctor]) -> bool {
        ctors.iter().all(|c| {
            matrix
                .iter()
                .any(|r| r.first().and_then(|h| h.ctor.as_ref()) == Some(c))
        })
    }

    /// Up to three missing patterns at the top level, for the message.
    fn missing(&self, matrix: &[Vec<DPat>], ty: Ty) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(ctors) = self.all_ctors(ty) {
            for ctor in ctors {
                let fields: Vec<DPat> = self
                    .ctor_fields(&ctor, ty)
                    .into_iter()
                    .map(DPat::wild)
                    .collect();
                let candidate = DPat {
                    ctor: Some(ctor.clone()),
                    fields,
                    ty,
                };
                if self.useful(matrix, &[candidate], 0) {
                    out.push(self.describe(&ctor, ty));
                }
                if out.len() == 3 {
                    break;
                }
            }
        }
        if out.is_empty() {
            out.push("_".to_owned());
        }
        out
    }

    fn describe(&self, ctor: &Ctor, ty: Ty) -> String {
        match ctor {
            Ctor::Bool(b) => b.to_string(),
            Ctor::Variant(v) => {
                let name = self.hir.def(*v).name.to_string();
                match self.items.variants.get(v) {
                    Some(VariantTy::Unit) | None => name,
                    Some(_) => format!("{name}(..)"),
                }
            }
            Ctor::Single => match self.interner.kind(ty) {
                TyKind::Tuple(ts) => format!("({})", vec!["_"; ts.len()].join(", ")),
                _ => "_".to_owned(),
            },
            Ctor::Literal(_) | Ctor::Const(_) => "_".to_owned(),
        }
    }
}

fn product(columns: Vec<Vec<DPat>>) -> Vec<Vec<DPat>> {
    let mut rows: Vec<Vec<DPat>> = vec![Vec::new()];
    for column in columns {
        let mut next = Vec::new();
        for row in &rows {
            for p in &column {
                let mut r = row.clone();
                r.push(p.clone());
                next.push(r);
            }
        }
        rows = next;
        if rows.len() > 1024 {
            rows.truncate(1024);
        }
    }
    rows
}

/// Checks every `case` without `else` and every pattern that must be irrefutable.
pub(crate) fn check_cases(
    view: &View<'_>,
    cases: &[ExprId],
    irrefutable: &[(PatId, &str)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for &case in cases {
        let Expr::Case {
            scrutinee, arms, ..
        } = view.body.expr(case)
        else {
            continue;
        };
        let _ = scrutinee;
        let mut matrix: Vec<Vec<DPat>> = Vec::new();
        let mut ty = None;
        for arm in arms {
            let rows = view.lower(arm.pat);
            ty = ty.or_else(|| rows.first().map(|r| r.ty));
            let reachable = rows
                .iter()
                .any(|r| view.useful(&matrix, std::slice::from_ref(r), 0));
            if !reachable {
                diagnostics.push(
                    Diagnostic::warning(
                        codes::UNREACHABLE_PATTERN,
                        view.body.pat_span(arm.pat),
                        "unreachable pattern",
                    )
                    .with_label("earlier arms already match every value this arm matches"),
                );
            }
            if arm.guard.is_none() {
                matrix.extend(rows.into_iter().map(|r| vec![r]));
            }
        }
        let Some(ty) = ty else {
            // A `case` with no arms is exhaustive only for `Never`.
            continue;
        };
        if view.useful(&matrix, &[DPat::wild(ty)], 0) {
            let missing = view.missing(&matrix, ty);
            let span = view.body.expr_span(case);
            let case_kw = loom_span::Span::new(
                span.file,
                loom_span::TextRange::at(span.range.start(), loom_span::TextSize::new(4)),
            );
            diagnostics.push(
                Diagnostic::error(
                    codes::NON_EXHAUSTIVE_CASE,
                    case_kw,
                    format!(
                        "`case` does not cover every value: missing {}",
                        missing
                            .iter()
                            .map(|m| format!("`{m}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .with_label("not exhaustive")
                .with_help("add arms for the missing patterns, or an `else` branch"),
            );
        }
    }
    for &(pat, place) in irrefutable {
        let rows = view.lower(pat);
        let Some(first) = rows.first() else { continue };
        let ty = first.ty;
        let matrix: Vec<Vec<DPat>> = rows.into_iter().map(|r| vec![r]).collect();
        if view.useful(&matrix, &[DPat::wild(ty)], 0) {
            diagnostics.push(
                Diagnostic::error(
                    codes::REFUTABLE_PATTERN,
                    view.body.pat_span(pat),
                    format!("refutable pattern in {place}"),
                )
                .with_label("this pattern does not match every value")
                .with_help("use `case` or `if … in …` to handle values that do not match"),
            );
        }
    }
}
