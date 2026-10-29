//! Resolution of names inside a body (spec §4.1, §7.2, §9.1, §9.3).

use crate::defmap::{DefMap, Namespace, PathError, Res, find_generic_param};
use loom_diagnostics::{Applicability, Diagnostic, Suggestion, codes};
use loom_hir::{
    BindingId, BindingKind, Body, DefId, DefKind, Expr, ExprId, Hir, Name, Pat, PatId, Path, Stmt,
};
use loom_span::{Span, TextRange};
use std::collections::{HashMap, HashSet};

/// What resolution learned about one body.
#[derive(Debug, Default)]
pub struct BodyResolution {
    /// Resolutions of `Expr::Name`, of `Expr::Field` that names a module member, and of
    /// `self`/`Self`.
    pub exprs: HashMap<ExprId, Res>,
    /// Capitalized names not found in scope: unqualified variants for the type checker
    /// to resolve against the expected type (spec §3.3 rule 2).
    pub unresolved_exprs: HashSet<ExprId>,
    /// Resolutions of path patterns.
    pub pats: HashMap<PatId, Res>,
    /// Single-segment path patterns not found in scope: variants of the matched enum.
    pub unresolved_pats: HashSet<PatId>,
    /// `x = e` statements that are assignments: the statement's binding → the existing
    /// `var` binding it assigns (spec §4.1).
    pub assignments: HashMap<BindingId, BindingId>,
    /// Bindings in the second and later alternatives of an or-pattern → the binding of
    /// the same name in the first alternative.
    pub aliases: HashMap<BindingId, BindingId>,
}

impl BodyResolution {
    /// The canonical binding for `binding` (following or-pattern aliases).
    #[must_use]
    pub fn canonical(&self, binding: BindingId) -> BindingId {
        self.aliases.get(&binding).copied().unwrap_or(binding)
    }
}

/// Re-exported for symmetry with `Res::Local`.
pub type LocalRes = BindingId;

pub(crate) fn resolve_body(
    hir: &Hir,
    map: &DefMap,
    body: &Body,
    diagnostics: &mut Vec<Diagnostic>,
) -> BodyResolution {
    let owner = hir.def(body.owner);
    let has_self = matches!(&owner.kind, DefKind::Fn(f) if f.receiver.is_some());
    let in_type = hir.generics_parent(body.owner).is_some();
    let mut walker = Walker {
        hir,
        map,
        body,
        module: owner.module,
        owner: body.owner,
        has_self,
        in_type,
        scopes: vec![Vec::new()],
        loops: vec![0],
        res: BodyResolution::default(),
        diagnostics,
    };
    for &param in &body.params {
        walker.declare(param);
    }
    for default in body.defaults.iter().flatten() {
        walker.expr(*default);
    }
    if let Some(root) = body.root {
        walker.expr(root);
    }
    walker.res
}

struct Walker<'a> {
    hir: &'a Hir,
    map: &'a DefMap,
    body: &'a Body,
    module: DefId,
    owner: DefId,
    has_self: bool,
    in_type: bool,
    scopes: Vec<Vec<(Name, BindingId)>>,
    /// Loop nesting depth, one entry per closure boundary.
    loops: Vec<usize>,
    res: BodyResolution,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Walker<'_> {
    fn span(&self, range: TextRange) -> Span {
        Span::new(self.body.file, range)
    }

    fn declare(&mut self, binding: BindingId) {
        let name = self.body.binding(binding).name.clone();
        if name.as_str() == "_" {
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.push((name, binding));
        }
    }

    fn lookup_local(&self, name: &Name) -> Option<BindingId> {
        self.scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .find(|(n, _)| n == name)
            .map(|&(_, b)| b)
    }

    fn with_scope(&mut self, bindings: Vec<(Name, BindingId)>, f: impl FnOnce(&mut Self)) {
        self.scopes.push(bindings);
        f(self);
        self.scopes.pop();
    }

    fn resolve_name(&mut self, id: ExprId, name: &Name) {
        if let Some(binding) = self.lookup_local(name) {
            self.res.exprs.insert(id, Res::Local(binding));
            return;
        }
        if let Some(param) = find_generic_param(self.hir, self.owner, name) {
            self.res.exprs.insert(id, Res::TyParam(param));
            return;
        }
        for ns in [Namespace::Values, Namespace::Types] {
            if let Some(def) = self.map.lookup_in_scope(self.hir, self.module, name, ns) {
                self.res.exprs.insert(id, Res::Def(def));
                return;
            }
        }
        if name.is_capitalized() {
            self.res.unresolved_exprs.insert(id);
            return;
        }
        self.res.exprs.insert(id, Res::Err);
        self.diagnostics.push(
            Diagnostic::error(
                codes::UNRESOLVED_NAME,
                self.body.expr_span(id),
                format!("cannot find `{name}` in this scope"),
            )
            .with_label("not found"),
        );
    }

    #[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
    fn expr(&mut self, id: ExprId) {
        match self.body.expr(id) {
            Expr::Missing | Expr::Literal(_) | Expr::Unsupported(_) => {}
            Expr::Name(name) => {
                let name = name.clone();
                self.resolve_name(id, &name);
            }
            Expr::SelfValue => {
                if self.has_self {
                    self.res.exprs.insert(id, Res::SelfValue);
                } else {
                    self.res.exprs.insert(id, Res::Err);
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::SELF_OUTSIDE_METHOD,
                            self.body.expr_span(id),
                            "`self` is only available in methods",
                        )
                        .with_label("not inside a method with a receiver"),
                    );
                }
            }
            Expr::SelfType => {
                if self.in_type {
                    self.res.exprs.insert(id, Res::SelfType);
                } else {
                    self.res.exprs.insert(id, Res::Err);
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::SELF_OUTSIDE_METHOD,
                            self.body.expr_span(id),
                            "`Self` is only available inside a type, trait or `impl`",
                        )
                        .with_label("not inside a type"),
                    );
                }
            }
            &Expr::Field {
                base,
                ref name,
                name_range,
            } => {
                let name = name.clone();
                self.expr(base);
                if let Some(Res::Def(module)) = self.res.exprs.get(&base).copied()
                    && matches!(self.hir.def(module).kind, DefKind::Module(_))
                {
                    self.module_member(id, module, &name, name_range);
                }
            }
            &Expr::TupleField { base, .. } | &Expr::Try(base) => self.expr(base),
            Expr::Call { callee, args } => {
                self.expr(*callee);
                for arg in args {
                    self.expr(arg.value);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.expr(*receiver);
                for arg in args {
                    self.expr(arg.value);
                }
            }
            Expr::Index { base, indices, .. } => {
                self.expr(*base);
                let base_is_type = match self.res.exprs.get(base) {
                    Some(Res::Def(def)) => crate::defmap::is_type_def(self.hir, *def),
                    Some(Res::TyParam(_) | Res::SelfType) => true,
                    _ => false,
                };
                if !base_is_type {
                    for &index in indices {
                        self.expr(index);
                    }
                }
            }
            &Expr::Unary { operand, .. } => self.expr(operand),
            &Expr::Binary { lhs, rhs, .. }
            | &Expr::Range {
                start: lhs,
                end: rhs,
                ..
            } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            &Expr::In { expr, pat } => {
                self.expr(expr);
                let mut bindings = Vec::new();
                self.pat(pat, &mut bindings);
                if let Some(&(_, first)) = bindings.first() {
                    let range = self.body.binding(first).range;
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::BINDING_PATTERN_TEST,
                            self.span(range),
                            "a pattern test can only bind names in an `if` or `while` condition",
                        )
                        .with_label("this binding could never be used")
                        .with_help("use `_` here, or move the test into an `if … in …` condition"),
                    );
                }
            }
            Expr::Interpolate(parts) => {
                for part in parts {
                    if let loom_hir::InterpolationPart::Expr(e) = part {
                        self.expr(*e);
                    }
                }
            }
            Expr::Tuple(elements) | Expr::Array(elements) => {
                for &e in elements {
                    self.expr(e);
                }
            }
            Expr::Map(entries) => {
                for &(k, v) in entries {
                    self.expr(k);
                    self.expr(v);
                }
            }
            Expr::Closure { params, body } => {
                let mut bindings = Vec::new();
                for param in params.iter().flatten() {
                    self.pat(param.pat, &mut bindings);
                }
                let body = *body;
                self.loops.push(0);
                self.with_scope(bindings, |w| w.expr(body));
                self.loops.pop();
            }
            Expr::Block { stmts } => {
                self.scopes.push(Vec::new());
                for stmt in stmts {
                    self.stmt(stmt);
                }
                self.scopes.pop();
            }
            &Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.scopes.push(Vec::new());
                let bound = self.condition(cond);
                self.with_scope(bound, |w| w.expr(then_branch));
                self.scopes.pop();
                if let Some(e) = else_branch {
                    self.expr(e);
                }
            }
            Expr::Select { arms, else_branch } => {
                for arm in arms {
                    self.expr(arm.operation);
                    let mut bindings = Vec::new();
                    if let Some(p) = arm.pat {
                        self.pat(p, &mut bindings);
                    }
                    let body = arm.body;
                    self.with_scope(bindings, |w| w.expr(body));
                }
                if let Some(e) = else_branch {
                    self.expr(*e);
                }
            }
            Expr::Case {
                scrutinee,
                arms,
                else_branch,
            } => {
                self.expr(*scrutinee);
                for arm in arms {
                    let mut bindings = Vec::new();
                    self.pat(arm.pat, &mut bindings);
                    let (guard, body) = (arm.guard, arm.body);
                    self.with_scope(bindings, |w| {
                        if let Some(g) = guard {
                            w.expr(g);
                        }
                        w.expr(body);
                    });
                }
                if let Some(e) = else_branch {
                    self.expr(*e);
                }
            }
            &Expr::Loop(body) => self.in_loop(|w| w.expr(body)),
            &Expr::While { cond, body } => {
                self.scopes.push(Vec::new());
                let bound = self.condition(cond);
                self.with_scope(bound, |w| w.in_loop(|w| w.expr(body)));
                self.scopes.pop();
            }
            &Expr::For { pat, iter, body } => {
                self.expr(iter);
                let mut bindings = Vec::new();
                self.pat(pat, &mut bindings);
                self.with_scope(bindings, |w| w.in_loop(|w| w.expr(body)));
            }
            Expr::Return(value) => {
                if let Some(v) = *value {
                    self.expr(v);
                }
            }
            Expr::Break(value) => {
                if let Some(v) = *value {
                    self.expr(v);
                }
                self.check_in_loop(id, "break");
            }
            Expr::Next => self.check_in_loop(id, "next"),
        }
    }

    fn in_loop(&mut self, f: impl FnOnce(&mut Self)) {
        if let Some(depth) = self.loops.last_mut() {
            *depth += 1;
        }
        f(self);
        if let Some(depth) = self.loops.last_mut() {
            *depth -= 1;
        }
    }

    fn check_in_loop(&mut self, id: ExprId, keyword: &str) {
        if self.loops.last().copied().unwrap_or(0) == 0 {
            self.diagnostics.push(
                Diagnostic::error(
                    codes::BREAK_OUTSIDE_LOOP,
                    self.body.expr_span(id),
                    format!("`{keyword}` outside of a loop"),
                )
                .with_label(format!(
                    "`{keyword}` can only be used inside `loop`, `while` or `for`"
                )),
            );
        }
    }

    fn module_member(&mut self, id: ExprId, module: DefId, name: &Name, range: TextRange) {
        let mut private = None;
        for ns in [Namespace::Values, Namespace::Types] {
            if let Some((def, vis)) = self.map.member(module, name, ns) {
                if crate::defmap::is_visible(self.hir, vis, module, self.module) {
                    self.res.exprs.insert(id, Res::Def(def));
                    return;
                }
                private = Some(def);
            }
        }
        self.res.exprs.insert(id, Res::Err);
        let module_name = self.hir.path_string(module);
        let diagnostic = if let Some(def) = private {
            Diagnostic::error(
                codes::PRIVATE_ITEM,
                self.span(range),
                format!("`{name}` is private to module `{module_name}`"),
            )
            .with_label("private")
            .with_secondary(self.hir.def(def).span, "declared here without `pub`")
        } else {
            Diagnostic::error(
                codes::UNRESOLVED_NAME,
                self.span(range),
                format!("cannot find `{name}` in module `{module_name}`"),
            )
            .with_label("not found")
        };
        self.diagnostics.push(diagnostic);
    }

    /// Resolves a condition, returning the bindings its pattern tests introduce
    /// (spec §9.3).
    fn condition(&mut self, cond: ExprId) -> Vec<(Name, BindingId)> {
        match *self.body.expr(cond) {
            Expr::In { expr, pat } => {
                self.expr(expr);
                let mut bindings = Vec::new();
                self.pat(pat, &mut bindings);
                bindings
            }
            Expr::Binary {
                op: loom_hir::BinaryOp::And,
                lhs,
                rhs,
                ..
            } => {
                let mut left = self.condition(lhs);
                self.scopes.push(left.clone());
                let right = self.condition(rhs);
                self.scopes.pop();
                left.extend(right);
                left
            }
            _ => {
                self.expr(cond);
                Vec::new()
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match *stmt {
            Stmt::Bind { binding, value, .. } => {
                self.expr(value);
                let data = self.body.binding(binding);
                match self.lookup_local(&data.name) {
                    Some(existing) if data.kind == BindingKind::Var => {
                        self.diagnostics.push(
                            Diagnostic::error(
                                codes::BINDING_REDECLARED,
                                self.span(data.range),
                                format!("`{}` is already declared in this function", data.name),
                            )
                            .with_label("bindings are never shadowed")
                            .with_secondary(
                                self.span(self.body.binding(existing).range),
                                "first declared here",
                            ),
                        );
                    }
                    Some(existing) => {
                        if self.body.binding(existing).kind.is_mutable() {
                            self.res.assignments.insert(binding, existing);
                        } else {
                            self.immutable_assignment(existing, data.range);
                        }
                    }
                    None => self.declare(binding),
                }
            }
            Stmt::Assign { place, value, .. } => {
                self.expr(place);
                self.expr(value);
                if let Some(Res::Local(existing)) = self.res.exprs.get(&place).copied()
                    && !self.body.binding(existing).kind.is_mutable()
                {
                    let range = self.body.expr_range(place);
                    self.immutable_assignment(existing, range);
                }
            }
            Stmt::Expr(e) => self.expr(e),
        }
    }

    fn immutable_assignment(&mut self, existing: BindingId, range: TextRange) {
        let binding = self.body.binding(existing);
        let mut diagnostic = Diagnostic::error(
            codes::ASSIGN_TO_IMMUTABLE,
            self.span(range),
            format!(
                "cannot assign twice to immutable binding `{}`",
                binding.name
            ),
        )
        .with_label("cannot assign twice")
        .with_secondary(self.span(binding.range), "first assigned here");
        if binding.kind == BindingKind::Let {
            diagnostic = diagnostic
                .with_help(format!(
                    "declare it with `var {}` to make it reassignable",
                    binding.name
                ))
                .with_suggestion(Suggestion::replace(
                    "make the binding reassignable",
                    self.span(TextRange::empty(binding.range.start())),
                    "var ",
                    Applicability::MachineApplicable,
                ));
        }
        self.diagnostics.push(diagnostic);
    }

    /// Resolves the paths in a pattern and collects its bindings.
    fn pat(&mut self, id: PatId, bindings: &mut Vec<(Name, BindingId)>) {
        match self.body.pat(id) {
            Pat::Missing | Pat::Wild | Pat::Rest | Pat::Literal { .. } | Pat::Range { .. } => {}
            &Pat::Bind(binding) => {
                let name = self.body.binding(binding).name.clone();
                if let Some(&(_, previous)) = bindings.iter().find(|(n, _)| *n == name) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::DUPLICATE_PATTERN_BINDING,
                            self.span(self.body.binding(binding).range),
                            format!("`{name}` is bound more than once in this pattern"),
                        )
                        .with_label("bound again here")
                        .with_secondary(
                            self.span(self.body.binding(previous).range),
                            "first bound here",
                        ),
                    );
                } else if name.as_str() != "_" {
                    bindings.push((name, binding));
                }
            }
            Pat::Tuple(elements) => {
                for &e in elements {
                    self.pat(e, bindings);
                }
            }
            Pat::Path(path) => self.pat_path(id, path),
            Pat::TupleStruct { path, pats, .. } => {
                self.pat_path(id, path);
                for &p in pats {
                    self.pat(p, bindings);
                }
            }
            Pat::Struct { path, fields, .. } => {
                self.pat_path(id, path);
                for &(_, _, p) in fields {
                    self.pat(p, bindings);
                }
            }
            Pat::Or(alternatives) => {
                let mut first: Option<Vec<(Name, BindingId)>> = None;
                for &alternative in alternatives {
                    let mut own = Vec::new();
                    self.pat(alternative, &mut own);
                    match &first {
                        None => first = Some(own),
                        Some(expected) => {
                            let mut names: Vec<&Name> = own.iter().map(|(n, _)| n).collect();
                            let mut want: Vec<&Name> = expected.iter().map(|(n, _)| n).collect();
                            names.sort();
                            want.sort();
                            if names == want {
                                for (name, binding) in &own {
                                    if let Some(&(_, canonical)) =
                                        expected.iter().find(|(n, _)| n == name)
                                    {
                                        self.res.aliases.insert(*binding, canonical);
                                    }
                                }
                            } else {
                                self.diagnostics.push(
                                    Diagnostic::error(
                                        codes::OR_PATTERN_BINDINGS,
                                        self.body.pat_span(alternative),
                                        "alternatives of an or-pattern must bind the same names",
                                    )
                                    .with_label(format!(
                                        "binds {}",
                                        if names.is_empty() {
                                            "nothing".to_owned()
                                        } else {
                                            names
                                                .iter()
                                                .map(|n| format!("`{n}`"))
                                                .collect::<Vec<_>>()
                                                .join(", ")
                                        }
                                    )),
                                );
                            }
                        }
                    }
                }
                bindings.extend(first.unwrap_or_default());
            }
        }
    }

    fn pat_path(&mut self, id: PatId, path: &Path) {
        if path.segments.is_empty() {
            return;
        }
        if path.segments.len() == 1 {
            let name = &path.segments[0].name;
            if let Some(def) =
                self.map
                    .lookup_in_scope(self.hir, self.module, name, Namespace::Values)
            {
                self.res.pats.insert(id, Res::Def(def));
            } else {
                self.res.unresolved_pats.insert(id);
            }
            return;
        }
        match self
            .map
            .resolve_path(self.hir, self.module, path, Namespace::Values)
        {
            Ok(def) => {
                self.res.pats.insert(id, Res::Def(def));
            }
            Err(PathError::NotFound(i) | PathError::Private(i, _)) => {
                self.res.pats.insert(id, Res::Err);
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::UNRESOLVED_NAME,
                        self.span(path.segments[i].range),
                        format!(
                            "cannot find `{}` in `{}`",
                            path.segments[i].name,
                            path.text()
                        ),
                    )
                    .with_label("not found"),
                );
            }
        }
    }
}
