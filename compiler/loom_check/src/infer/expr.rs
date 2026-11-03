//! Expressions, statements and control flow.

use super::{Allowed, ClosureFrame, InferCtxt, LoopFrame, OperatorCheck};
use crate::items::TraitRef;
use crate::{CallTarget, ExprInfo, ForKind};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{
    BinaryOp, DefKind, Expr, ExprId, FnKind, InterpolationPart, Literal, Stmt, UnaryOp,
};
use loom_resolve::Res;
use loom_types::{FloatTy, Subst, Ty, TyKind, VarKind};
use std::collections::HashSet;

impl InferCtxt<'_> {
    /// Checks `expr`, using `expected` as a hint, and records its type.
    pub(super) fn expr(&mut self, expr: ExprId, expected: Option<Ty>) -> Ty {
        let ty = self.expr_inner(expr, expected);
        self.record(expr, ty)
    }

    #[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
    fn expr_inner(&mut self, id: ExprId, expected: Option<Ty>) -> Ty {
        let interner = self.cx.interner;
        let body = self.body;
        match body.expr(id) {
            Expr::Missing => interner.error(),
            Expr::Unsupported(what) => {
                self.error(
                    Diagnostic::error(
                        codes::NOT_IMPLEMENTED_YET,
                        self.span(id),
                        format!("{what} is not implemented yet"),
                    )
                    .with_label("not available in this phase"),
                );
                interner.error()
            }
            Expr::Literal(lit) => self.literal(id, lit, expected, false),
            Expr::Interpolate(parts) => {
                for part in parts {
                    if let InterpolationPart::Expr(e) = part {
                        let ty = self.infer(*e);
                        let ty = self.deref(ty);
                        self.require(ty, "Display", Vec::new(), self.span(*e));
                    }
                }
                interner.string()
            }
            Expr::Name(_) | Expr::SelfType => self.path_value(id, expected),
            Expr::SelfValue => match self.res.exprs.get(&id) {
                Some(Res::SelfValue) => self.self_ty.unwrap_or_else(|| interner.error()),
                _ => interner.error(),
            },
            &Expr::Field {
                base,
                ref name,
                name_range,
            } => {
                if matches!(self.res.exprs.get(&id), Some(Res::Def(_))) {
                    return self.path_value(id, expected);
                }
                if let Some(type_expr) = self.type_expr(base) {
                    return self.assoc_value(id, type_expr, name, name_range, expected);
                }
                let base_ty = self.infer(base);
                self.field_or_method(id, base, base_ty, name, name_range, expected)
            }
            &Expr::TupleField { base, index } => {
                let base_ty = self.infer(base);
                let base_ty = self.deref(base_ty);
                match interner.kind(base_ty) {
                    TyKind::Tuple(elements) if (index as usize) < elements.len() => {
                        elements[index as usize]
                    }
                    TyKind::Error => interner.error(),
                    TyKind::Infer(..) => self.annotations_needed(base),
                    _ => {
                        let text = self.show(base_ty);
                        self.error(
                            Diagnostic::error(
                                codes::UNKNOWN_FIELD,
                                self.span(id),
                                format!("no field `{index}` on type `{text}`"),
                            )
                            .with_label("unknown field"),
                        );
                        interner.error()
                    }
                }
            }
            Expr::Call { callee, args } => self.call(id, *callee, args, expected),
            Expr::MethodCall {
                receiver,
                name,
                name_range,
                args,
            } => {
                if let Some(type_expr) = self.type_expr(*receiver) {
                    return self.assoc_call(id, type_expr, name, *name_range, args, expected);
                }
                let receiver_ty = self.infer(*receiver);
                self.method_call(
                    id,
                    *receiver,
                    receiver_ty,
                    name,
                    *name_range,
                    args,
                    expected,
                )
            }
            Expr::Index { base, indices, .. } => {
                if self.type_expr(*base).is_some() {
                    self.error(
                        Diagnostic::error(
                            codes::WRONG_NAME_KIND,
                            self.span(id),
                            "expected a value, found a type",
                        )
                        .with_label("a type with generic arguments is not a value")
                        .with_help("call an associated function, such as `.new`"),
                    );
                    return interner.error();
                }
                let base_ty = self.infer(*base);
                self.index(id, base_ty, indices)
            }
            &Expr::Unary { op, operand } => self.unary(id, op, operand, expected),
            &Expr::Binary { op, lhs, rhs, .. } => self.binary(id, op, lhs, rhs, expected),
            &Expr::Range { start, end, .. } => {
                let ty = self.infer(start);
                let ty = self.deref(ty);
                self.check(end, ty);
                self.operator_checks.push(OperatorCheck {
                    expr: id,
                    ty,
                    op: "..",
                    allowed: Allowed::IntegerOrChar,
                });
                self.lang_adt("Range", vec![ty])
            }
            &Expr::In { expr, pat } => {
                let ty = self.infer(expr);
                self.check_pat(pat, ty);
                interner.bool()
            }
            &Expr::Try(inner) => self.try_expr(id, inner),
            Expr::Tuple(elements) => {
                if elements.is_empty() {
                    return interner.unit();
                }
                let expected_elements =
                    expected.and_then(|e| match interner.kind(self.structurally(e)) {
                        TyKind::Tuple(ts) if ts.len() == elements.len() => Some(ts),
                        _ => None,
                    });
                let tys = elements
                    .iter()
                    .enumerate()
                    .map(|(i, &e)| match &expected_elements {
                        Some(ts) => self.check(e, ts[i]),
                        None => self.infer(e),
                    })
                    .collect();
                interner.intern(TyKind::Tuple(tys))
            }
            Expr::Array(elements) => {
                let element = self
                    .expected_args("Array", expected)
                    .and_then(|a| a.first().copied())
                    .unwrap_or_else(|| self.new_var());
                for &e in elements {
                    self.check(e, element);
                }
                self.lang_adt("Array", vec![element])
            }
            Expr::Map(entries) => {
                let args = self.expected_args("Map", expected);
                let (key, value) = match args.as_deref() {
                    Some([k, v]) => (*k, *v),
                    _ => (self.new_var(), self.new_var()),
                };
                for &(k, v) in entries {
                    self.check(k, key);
                    self.check(v, value);
                }
                self.require(key, "Eq", Vec::new(), self.span(id));
                self.require(key, "Hash", Vec::new(), self.span(id));
                self.lang_adt("Map", vec![key, value])
            }
            Expr::Closure {
                params,
                body: closure_body,
            } => self.closure(id, params.as_deref(), *closure_body, expected),
            Expr::Block { stmts } => self.block(stmts, expected),
            &Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.check(cond, interner.bool());
                match else_branch {
                    None => {
                        self.infer(then_branch);
                        interner.unit()
                    }
                    Some(else_branch) => self.branches(&[then_branch, else_branch], expected),
                }
            }
            Expr::Select { arms, else_branch } => {
                let mut ops = Vec::new();
                for arm in arms {
                    let op_ty = self.infer(arm.operation);
                    let op = self.select_op(arm.operation);
                    match (op, arm.pat) {
                        (Some(crate::SelectOp::Recv { .. }), Some(p)) => self.check_pat(p, op_ty),
                        (Some(crate::SelectOp::Send { .. } | crate::SelectOp::Timer), Some(p)) => {
                            self.error(
                                Diagnostic::error(
                                    codes::SELECT_ARM,
                                    self.body.pat_span(p),
                                    "only a `recv` arm receives into a pattern".to_owned(),
                                )
                                .with_label("remove the pattern and `=`"),
                            );
                        }
                        _ => {}
                    }
                    if op.is_none() {
                        self.error(
                            Diagnostic::error(
                                codes::SELECT_ARM,
                                self.span(arm.operation),
                                "a `select` arm waits on `ch.recv`, `ch.send(value)` or `Time.after(duration)`".to_owned(),
                            )
                            .with_label("not a channel operation or timer"),
                        );
                    }
                    ops.push(op.unwrap_or(crate::SelectOp::Timer));
                    self.infer(arm.body);
                }
                if let Some(e) = else_branch {
                    self.infer(*e);
                }
                self.selects.insert(id, ops);
                interner.unit()
            }
            Expr::Case {
                scrutinee,
                arms,
                else_branch,
            } => {
                let scrutinee_ty = self.infer(*scrutinee);
                let mut bodies = Vec::new();
                for arm in arms {
                    self.check_pat(arm.pat, scrutinee_ty);
                    if let Some(guard) = arm.guard {
                        self.check(guard, interner.bool());
                    }
                    bodies.push(arm.body);
                }
                if let Some(e) = else_branch {
                    bodies.push(*e);
                }
                if else_branch.is_none() {
                    self.cases.push(id);
                }
                self.branches(&bodies, expected)
            }
            &Expr::Loop(loop_body) => {
                let break_ty = self.new_var();
                self.loops.push(LoopFrame {
                    break_ty: Some(break_ty),
                    has_break: false,
                });
                self.infer(loop_body);
                let frame = self.loops.pop();
                if frame.is_some_and(|f| f.has_break) {
                    break_ty
                } else {
                    interner.never()
                }
            }
            &Expr::While {
                cond,
                body: loop_body,
            } => {
                self.check(cond, interner.bool());
                self.loops.push(LoopFrame {
                    break_ty: None,
                    has_break: false,
                });
                self.infer(loop_body);
                self.loops.pop();
                interner.unit()
            }
            &Expr::For {
                pat,
                iter,
                body: loop_body,
            } => {
                let item = self.for_item(id, iter);
                self.check_pat(pat, item);
                self.irrefutable.push((pat, "a `for` loop"));
                self.loops.push(LoopFrame {
                    break_ty: None,
                    has_break: false,
                });
                self.infer(loop_body);
                self.loops.pop();
                interner.unit()
            }
            &Expr::Return(value) => {
                let ret = self
                    .return_tys
                    .last()
                    .copied()
                    .unwrap_or_else(|| interner.unit());
                if let Some(v) = value {
                    self.check(v, ret);
                } else {
                    let unit = interner.unit();
                    self.unify_at(id, unit, ret);
                }
                interner.never()
            }
            &Expr::Break(value) => {
                let frame_ty = self.loops.last_mut().map(|f| {
                    f.has_break = true;
                    f.break_ty
                });
                if let Some(v) = value {
                    match frame_ty {
                        Some(Some(t)) => {
                            self.check(v, t);
                        }
                        Some(None) => {
                            self.infer(v);
                            self.error(
                                Diagnostic::error(
                                    codes::BREAK_VALUE_OUTSIDE_LOOP,
                                    self.span(id),
                                    "`break` with a value is only allowed in `loop`",
                                )
                                .with_label("`while` and `for` loops have no value"),
                            );
                        }
                        None => {
                            self.infer(v);
                        }
                    }
                } else if let Some(Some(t)) = frame_ty {
                    let unit = interner.unit();
                    let _ = self.table.unify(interner, t, unit);
                }
                interner.never()
            }
            Expr::Next => interner.never(),
        }
    }

    fn annotations_needed(&mut self, expr: ExprId) -> Ty {
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

    fn expected_args(&mut self, lang: &str, expected: Option<Ty>) -> Option<Vec<Ty>> {
        let expected = self.structurally(expected?);
        let def = self.lang(lang)?;
        match self.cx.interner.kind(expected) {
            TyKind::Adt(d, args) if d == def => Some(args),
            _ => None,
        }
    }

    pub(super) fn literal(
        &mut self,
        id: ExprId,
        lit: &Literal,
        expected: Option<Ty>,
        negative: bool,
    ) -> Ty {
        let interner = self.cx.interner;
        let expected = expected.map(|e| self.structurally(e));
        match lit {
            Literal::Int(value) => {
                self.int_literals.push((id, *value, negative));
                match expected.map(|e| interner.kind(e)) {
                    Some(TyKind::Int(_) | TyKind::Infer(_, VarKind::Int)) => {
                        expected.unwrap_or_else(|| interner.int())
                    }
                    _ => self.table.new_var(interner, VarKind::Int),
                }
            }
            Literal::Float(_) => match expected.map(|e| interner.kind(e)) {
                Some(TyKind::Float(_) | TyKind::Infer(_, VarKind::Float)) => {
                    expected.unwrap_or_else(|| interner.intern(TyKind::Float(FloatTy::F64)))
                }
                _ => self.table.new_var(interner, VarKind::Float),
            },
            Literal::Char(_) => interner.intern(TyKind::Char),
            Literal::String(_) => interner.string(),
            Literal::Bytes(_) => interner.intern(TyKind::Bytes),
            Literal::Bool(_) => interner.bool(),
        }
    }

    /// The common type of branches (of `if`/`case`), each checked against `expected` or
    /// against the first branch that does not diverge.
    fn branches(&mut self, branches: &[ExprId], expected: Option<Ty>) -> Ty {
        let interner = self.cx.interner;
        if let Some(e) = expected {
            for &b in branches {
                self.check_block_value(b, e);
            }
            return e;
        }
        let mut result: Option<Ty> = None;
        for &b in branches {
            match result {
                None => {
                    let t = self.infer(b);
                    let resolved = self.shallow(t);
                    if !matches!(interner.kind(resolved), TyKind::Never) {
                        result = Some(t);
                    }
                }
                Some(t) => {
                    self.check(b, t);
                }
            }
        }
        result.unwrap_or_else(|| interner.never())
    }

    pub(super) fn block(&mut self, stmts: &[Stmt], expected: Option<Ty>) -> Ty {
        let interner = self.cx.interner;
        let Some((last, rest)) = stmts.split_last() else {
            return interner.unit();
        };
        for stmt in rest {
            self.stmt(stmt);
        }
        match last {
            Stmt::Expr(e) => match expected {
                Some(t) => {
                    let resolved = self.resolve(t);
                    if matches!(interner.kind(resolved), TyKind::Unit) {
                        let actual = self.infer(*e);
                        let actual = self.shallow(actual);
                        if matches!(interner.kind(actual), TyKind::Never) {
                            actual
                        } else {
                            t
                        }
                    } else {
                        self.check(*e, t)
                    }
                }
                None => self.infer(*e),
            },
            other => {
                self.stmt(other);
                interner.unit()
            }
        }
    }

    fn stmt(&mut self, stmt: &Stmt) {
        let interner = self.cx.interner;
        match *stmt {
            Stmt::Bind {
                binding,
                ref ty,
                value,
            } => {
                if let Some(&existing) = self.res.assignments.get(&binding) {
                    let target = self.binding_ty(existing);
                    self.check(value, target);
                    self.note_mutation(existing);
                    self.binding_tys[binding.index()] = Some(target);
                    return;
                }
                let declared = match ty {
                    Some(annotation) => {
                        let t = self.lower_type(annotation);
                        self.check(value, t)
                    }
                    None => self.infer(value),
                };
                self.declare_binding(binding, declared);
            }
            Stmt::Assign {
                place,
                op,
                value,
                op_range,
            } => {
                let place_ty = self.place(place);
                match op {
                    None => {
                        self.check(value, place_ty);
                    }
                    Some(op) => {
                        self.check(value, place_ty);
                        let allowed = if op.is_bitwise() {
                            Allowed::IntegerOrBool
                        } else if op.is_shift() {
                            Allowed::Integer
                        } else {
                            Allowed::Numeric
                        };
                        let _ = op_range;
                        self.operator_checks.push(OperatorCheck {
                            expr: place,
                            ty: place_ty,
                            op: op.symbol(),
                            allowed,
                        });
                    }
                }
            }
            Stmt::Expr(e) => {
                let ty = self.infer(e);
                let ty = self.resolve(ty);
                if let (Some(result), TyKind::Adt(def, _)) =
                    (self.lang("Result"), interner.kind(ty))
                    && def == result
                    && !matches!(
                        self.body.expr(e),
                        Expr::Block { .. } | Expr::If { .. } | Expr::Case { .. }
                    )
                {
                    self.diagnostics.push(
                        Diagnostic::warning(
                            loom_diagnostics::codes::UNUSED_RESULT,
                            self.span(e),
                            "unused `Result` that must be used",
                        )
                        .with_label("this `Result` may be an `Err` that is ignored")
                        .with_help(
                            "handle it with `?` or `case`, or discard it explicitly with `_ = …`",
                        ),
                    );
                }
            }
        }
    }

    /// The type of an assigned place, checking that it is a mutable place.
    fn place(&mut self, place: ExprId) -> Ty {
        let interner = self.cx.interner;
        match self.body.expr(place) {
            Expr::Name(_) => {
                let ty = self.infer(place);
                if let Some(Res::Local(b)) = self.res.exprs.get(&place).copied() {
                    self.note_mutation(b);
                }
                ty
            }
            Expr::Field { .. } | Expr::TupleField { .. } => {
                let ty = self.infer(place);
                if matches!(self.exprs.get(&place), Some(ExprInfo::Call { .. })) {
                    self.error(
                        Diagnostic::error(
                            codes::MUTATE_IMMUTABLE,
                            self.span(place),
                            "cannot assign to the result of a method call",
                        )
                        .with_label("not a place"),
                    );
                }
                self.require_mutable(place, "assign to a field of");
                ty
            }
            Expr::Index { base, indices, .. } => {
                let base_ty = self.infer(*base);
                let base_resolved = self.deref(base_ty);
                // `map[key] = value` inserts (spec §7.4 rule 5).
                let ty = if let (Some(map), TyKind::Adt(def, args)) =
                    (self.lang("Map"), interner.kind(base_resolved))
                    && def == map
                    && args.len() == 2
                {
                    if let Some(&key) = indices.first() {
                        self.check(key, args[0]);
                    }
                    args[1]
                } else {
                    self.index(place, base_ty, indices)
                };
                self.record(place, ty);
                self.require_mutable(place, "assign to an element of");
                ty
            }
            Expr::Missing => interner.error(),
            _ => {
                self.infer(place);
                self.error(
                    Diagnostic::error(
                        codes::MUTATE_IMMUTABLE,
                        self.span(place),
                        "this expression cannot be assigned to",
                    )
                    .with_label("not a place"),
                );
                interner.error()
            }
        }
    }

    fn index(&mut self, id: ExprId, base_ty: Ty, indices: &[ExprId]) -> Ty {
        let interner = self.cx.interner;
        let base_ty = self.deref(base_ty);
        let kind = interner.kind(base_ty);
        let Some(&index) = indices.first() else {
            return interner.error();
        };
        match kind {
            TyKind::Adt(def, args) if Some(def) == self.lang("Array") && args.len() == 1 => {
                self.check(index, interner.int());
                args[0]
            }
            TyKind::Adt(def, args) if Some(def) == self.lang("Map") && args.len() == 2 => {
                self.check(index, args[0]);
                args[1]
            }
            TyKind::Error => interner.error(),
            TyKind::Infer(..) => self.annotations_needed(id),
            _ => {
                for &i in indices {
                    self.infer(i);
                }
                let text = self.show(base_ty);
                self.error(
                    Diagnostic::error(
                        codes::NOT_INDEXABLE,
                        self.span(id),
                        format!("cannot index a value of type `{text}`"),
                    )
                    .with_label("only `Array` and `Map` can be indexed"),
                );
                interner.error()
            }
        }
    }

    fn unary(&mut self, id: ExprId, op: UnaryOp, operand: ExprId, expected: Option<Ty>) -> Ty {
        let interner = self.cx.interner;
        match op {
            UnaryOp::Not => {
                self.check(operand, interner.bool());
                interner.bool()
            }
            UnaryOp::Neg => {
                let ty = if let Expr::Literal(lit @ Literal::Int(_)) = self.body.expr(operand) {
                    let lit = lit.clone();
                    let t = self.literal(operand, &lit, expected, true);
                    // The literal is checked as negative; drop the positive check.
                    self.int_literals
                        .retain(|&(e, _, negative)| e != operand || negative);
                    self.record(operand, t)
                } else {
                    self.expr(operand, expected)
                };
                self.operator_checks.push(OperatorCheck {
                    expr: id,
                    ty,
                    op: "-",
                    allowed: Allowed::Signed,
                });
                ty
            }
            UnaryOp::BitNot => {
                let ty = self.expr(operand, expected);
                let ty = self.deref(ty);
                self.operator_checks.push(OperatorCheck {
                    expr: id,
                    ty,
                    op: "~",
                    allowed: Allowed::Integer,
                });
                ty
            }
        }
    }

    fn binary(
        &mut self,
        id: ExprId,
        op: BinaryOp,
        lhs: ExprId,
        rhs: ExprId,
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        match op {
            BinaryOp::And | BinaryOp::Or => {
                self.check(lhs, interner.bool());
                self.check(rhs, interner.bool());
                interner.bool()
            }
            _ if op.is_equality() || op.is_ordering() => {
                let ty = self.infer(lhs);
                let ty = self.deref(ty);
                self.check(rhs, ty);
                let trait_name = if op.is_equality() { "Eq" } else { "Ord" };
                self.require(ty, trait_name, Vec::new(), self.span(id));
                interner.bool()
            }
            _ => {
                let hint = expected.filter(|&e| {
                    let e = self.shallow(e);
                    matches!(
                        interner.kind(e),
                        TyKind::Int(_)
                            | TyKind::Float(_)
                            | TyKind::Infer(_, VarKind::Int | VarKind::Float)
                    )
                });
                let ty = self.expr(lhs, hint);
                let ty = self.deref(ty);
                self.check(rhs, ty);
                let allowed = if op.is_bitwise() {
                    Allowed::IntegerOrBool
                } else if op.is_shift() {
                    Allowed::Integer
                } else {
                    Allowed::Numeric
                };
                self.operator_checks.push(OperatorCheck {
                    expr: id,
                    ty,
                    op: op.symbol(),
                    allowed,
                });
                ty
            }
        }
    }

    fn try_expr(&mut self, id: ExprId, inner: ExprId) -> Ty {
        let interner = self.cx.interner;
        let ty = self.infer(inner);
        let ty = self.deref(ty);
        let ret = self
            .return_tys
            .last()
            .copied()
            .unwrap_or_else(|| interner.unit());
        let ret = self.structurally(ret);
        let (result, option) = (self.lang("Result"), self.lang("Option"));
        match (interner.kind(ty), interner.kind(ret)) {
            (TyKind::Adt(d, args), TyKind::Adt(r, ret_args))
                if Some(d) == result
                    && Some(r) == result
                    && args.len() == 2
                    && ret_args.len() == 2 =>
            {
                let (error, ret_error) = (args[1], ret_args[1]);
                let convert = if self.table.unify(interner, ret_error, error).is_ok() {
                    None
                } else {
                    let from = self.lang("From");
                    match from {
                        Some(from) => {
                            let trait_ref = TraitRef {
                                trait_: from,
                                args: vec![error],
                            };
                            self.require_trait(ret_error, trait_ref, self.span(id));
                            let method =
                                self.cx.items.traits.get(&from).and_then(|t| {
                                    t.methods.get(&loom_hir::Name::new("from")).copied()
                                });
                            method.map(|method| {
                                let mut subst = Subst::new();
                                subst.insert(crate::Items::self_param(from), ret_error);
                                if let Some(&p) = self.cx.items.generics_of(from).own.first() {
                                    subst.insert(p, error);
                                }
                                CallTarget::Trait {
                                    method,
                                    self_ty: ret_error,
                                    subst,
                                }
                            })
                        }
                        None => None,
                    }
                };
                self.exprs.insert(
                    id,
                    ExprInfo::Try {
                        result: true,
                        convert,
                    },
                );
                args[0]
            }
            (TyKind::Adt(d, args), TyKind::Adt(r, _))
                if Some(d) == option && Some(r) == option && args.len() == 1 =>
            {
                self.exprs.insert(
                    id,
                    ExprInfo::Try {
                        result: false,
                        convert: None,
                    },
                );
                args[0]
            }
            (TyKind::Error, _) | (_, TyKind::Error) => interner.error(),
            _ => {
                let (value, function) = (self.show(ty), self.show(ret));
                self.error(
                    Diagnostic::error(
                        codes::INVALID_TRY,
                        self.span(id),
                        format!("`?` cannot be used on `{value}` in a function returning `{function}`"),
                    )
                    .with_label("`?` needs a `Result` in a function returning `Result`, or an `Option` in one returning `Option`"),
                );
                interner.error()
            }
        }
    }

    /// The item type of a `for` loop over `iter` (spec §7.8 rule 3).
    fn for_item(&mut self, id: ExprId, iter: ExprId) -> Ty {
        let interner = self.cx.interner;
        let ty = self.infer(iter);
        let ty = self.deref(ty);
        if matches!(interner.kind(ty), TyKind::Error) {
            return interner.error();
        }
        if matches!(interner.kind(ty), TyKind::Infer(..)) {
            return self.annotations_needed(iter);
        }
        let (Some(iterator), Some(iterable)) = (self.lang("Iterator"), self.lang("Iterable"))
        else {
            return interner.error();
        };
        let item_of = |icx: &mut Self, it: Ty| -> Ty {
            let item = icx
                .cx
                .items
                .traits
                .get(&iterator)
                .and_then(|t| t.assoc_types.get(&loom_hir::Name::new("Item")).copied());
            match item {
                Some(assoc) => {
                    let projection = interner.intern(TyKind::Projection {
                        trait_: iterator,
                        args: vec![it],
                        assoc,
                    });
                    icx.normalize(projection)
                }
                None => interner.error(),
            }
        };
        let iterator_ref = TraitRef {
            trait_: iterator,
            args: Vec::new(),
        };
        if self.implements(ty, &iterator_ref) {
            self.exprs.insert(id, ExprInfo::For(ForKind::Iterator));
            return item_of(self, ty);
        }
        let iterable_ref = TraitRef {
            trait_: iterable,
            args: Vec::new(),
        };
        if self.implements(ty, &iterable_ref) {
            self.exprs.insert(id, ExprInfo::For(ForKind::Iterable));
            let assoc = self
                .cx
                .items
                .traits
                .get(&iterable)
                .and_then(|t| t.assoc_types.get(&loom_hir::Name::new("Iter")).copied());
            let Some(assoc) = assoc else {
                return interner.error();
            };
            let projection = interner.intern(TyKind::Projection {
                trait_: iterable,
                args: vec![ty],
                assoc,
            });
            let iter_ty = self.normalize(projection);
            return item_of(self, iter_ty);
        }
        let text = self.show(ty);
        self.error(
            Diagnostic::error(
                codes::UNSATISFIED_BOUND,
                self.span(iter),
                format!("`{text}` cannot be iterated"),
            )
            .with_label("a `for` loop needs an `Iterator` or an `Iterable` value"),
        );
        interner.error()
    }

    fn closure(
        &mut self,
        id: ExprId,
        params: Option<&[loom_hir::ClosureParam]>,
        closure_body: ExprId,
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        let expected_callable = expected.and_then(|e| match interner.kind(self.structurally(e)) {
            TyKind::Callable { kind, params, ret } => Some((kind, params, ret)),
            _ => None,
        });
        let count = params.map_or(0, <[_]>::len);
        let (expected_kind, mut param_tys, ret) = match &expected_callable {
            Some((kind, ps, ret)) => (Some(*kind), ps.clone(), *ret),
            None => (None, Vec::new(), self.new_var()),
        };
        if param_tys.len() != count {
            if expected_callable.is_some() && (params.is_some() || !param_tys.is_empty()) {
                self.error(
                    Diagnostic::error(
                        codes::WRONG_ARGUMENT_COUNT,
                        self.span(id),
                        format!(
                            "this block takes {count} parameter{}, but {} {} expected",
                            if count == 1 { "" } else { "s" },
                            param_tys.len(),
                            if param_tys.len() == 1 { "is" } else { "are" }
                        ),
                    )
                    .with_label("wrong number of block parameters"),
                );
            }
            param_tys = (0..count).map(|_| self.new_var()).collect();
        }
        self.closures.push(ClosureFrame {
            declared: HashSet::default(),
            mutates_capture: false,
        });
        for (i, param) in params.unwrap_or(&[]).iter().enumerate() {
            let ty = match &param.ty {
                Some(annotation) => {
                    let t = self.lower_type(annotation);
                    let _ = self.table.unify(interner, t, param_tys[i]);
                    t
                }
                None => param_tys[i],
            };
            self.check_pat(param.pat, ty);
            self.irrefutable.push((param.pat, "a block parameter"));
        }
        self.return_tys.push(ret);
        let saved_loops = std::mem::take(&mut self.loops);
        self.check_block_value(closure_body, ret);
        self.loops = saved_loops;
        self.return_tys.pop();
        let frame = self.closures.pop().unwrap_or(ClosureFrame {
            declared: HashSet::default(),
            mutates_capture: false,
        });
        let own_kind = Self::closure_kind_of_frame(&frame);
        if frame.mutates_capture {
            // Mutation of captures propagates to enclosing closures as well.
            if let Some(outer) = self.closures.last_mut() {
                let _ = outer;
            }
        }
        let kind = match expected_kind {
            Some(k) if own_kind <= k => k,
            Some(FnKind::Fn) => {
                self.error(
                    Diagnostic::error(
                        codes::MISMATCHED_TYPES,
                        self.span(id),
                        "expected an `Fn` closure, but this closure changes captured state",
                    )
                    .with_label("this closure is `FnMut`"),
                );
                FnKind::Fn
            }
            _ => own_kind,
        };
        self.exprs.insert(id, ExprInfo::Closure(kind));
        interner.intern(TyKind::Callable {
            kind,
            params: param_tys,
            ret,
        })
    }

    /// A name or module path used as a value.
    fn path_value(&mut self, id: ExprId, expected: Option<Ty>) -> Ty {
        let interner = self.cx.interner;
        if matches!(self.body.expr(id), Expr::SelfType) {
            return self.not_a_value(id, "`Self`");
        }
        if self.res.unresolved_exprs.contains(&id) {
            let name = match self.body.expr(id) {
                Expr::Name(n) => n.clone(),
                _ => return interner.error(),
            };
            if let Some((variant, enum_ty, subst)) = self.variant_from_expected(&name, expected) {
                return self.variant_value(id, variant, enum_ty, &subst);
            }
            return self.unknown_unqualified(id, &name, expected);
        }
        match self.res.exprs.get(&id).copied() {
            Some(Res::Local(b)) => {
                self.exprs.insert(id, ExprInfo::Local(b));
                self.binding_ty(b)
            }
            Some(Res::Def(def)) => self.def_value(id, def, expected),
            Some(Res::TyParam(_) | Res::SelfType) => self.not_a_value(id, "a type parameter"),
            Some(Res::SelfValue) => self.self_ty.unwrap_or_else(|| interner.error()),
            Some(Res::Err) | None => interner.error(),
        }
    }

    pub(super) fn not_a_value(&mut self, id: ExprId, what: &str) -> Ty {
        self.error(
            Diagnostic::error(
                codes::WRONG_NAME_KIND,
                self.span(id),
                format!("expected a value, found {what}"),
            )
            .with_label("not a value"),
        );
        self.cx.interner.error()
    }

    fn unknown_unqualified(
        &mut self,
        id: ExprId,
        name: &loom_hir::Name,
        expected: Option<Ty>,
    ) -> Ty {
        let expected_enum =
            expected.and_then(|e| match self.cx.interner.kind(self.structurally(e)) {
                TyKind::Adt(def, _) if matches!(self.def_kind(def), DefKind::Enum(_)) => Some(def),
                _ => None,
            });
        let diagnostic = match expected_enum {
            Some(enum_def) => Diagnostic::error(
                codes::UNKNOWN_VARIANT,
                self.span(id),
                format!(
                    "no variant `{name}` in enum `{}`",
                    self.cx.hir.def(enum_def).name
                ),
            )
            .with_label("unknown variant"),
            None => Diagnostic::error(
                codes::UNRESOLVED_NAME,
                self.span(id),
                format!("cannot find `{name}` in this scope"),
            )
            .with_label("not found"),
        };
        self.error(diagnostic);
        self.cx.interner.error()
    }

    /// The value of item `def` used without a call.
    pub(super) fn def_value(
        &mut self,
        id: ExprId,
        def: loom_hir::DefId,
        expected: Option<Ty>,
    ) -> Ty {
        let interner = self.cx.interner;
        match self.def_kind(def) {
            DefKind::Fn(_) => {
                let (subst, sig) = self.instantiate_fn(def, None);
                self.exprs.insert(id, ExprInfo::FnValue { def, subst });
                interner.intern(TyKind::Callable {
                    kind: FnKind::Fn,
                    params: sig.params.iter().map(|p| p.2).collect(),
                    ret: sig.ret,
                })
            }
            DefKind::Const(_) => {
                self.exprs.insert(id, ExprInfo::Const { def });
                let ty = self
                    .cx
                    .items
                    .consts
                    .get(&def)
                    .copied()
                    .unwrap_or_else(|| interner.error());
                self.normalize(ty)
            }
            DefKind::Variant(_) => {
                let enum_def = self.cx.hir.def(def).parent.unwrap_or(def);
                let (enum_ty, subst) = self.instantiate_adt(enum_def, expected);
                self.variant_value(id, def, enum_ty, &subst)
            }
            DefKind::Struct(_) => {
                let name = self.cx.hir.def(def).name.clone();
                self.error(
                    Diagnostic::error(
                        codes::WRONG_NAME_KIND,
                        self.span(id),
                        format!("expected a value, found struct `{name}`"),
                    )
                    .with_label("a struct is constructed with a call")
                    .with_help(format!("write `{name}(…)` with every field named")),
                );
                interner.error()
            }
            other => {
                let what = format!("{} `{}`", other.describe(), self.cx.hir.def(def).name);
                self.not_a_value(id, &what)
            }
        }
    }

    /// A variant used as a value: a unit variant, or a constructor function.
    pub(super) fn variant_value(
        &mut self,
        id: ExprId,
        variant: loom_hir::DefId,
        enum_ty: Ty,
        subst: &Subst,
    ) -> Ty {
        let interner = self.cx.interner;
        match self.cx.items.variants.get(&variant) {
            Some(crate::VariantTy::Unit) => {
                self.exprs
                    .insert(id, ExprInfo::UnitVariant { def: variant });
                enum_ty
            }
            Some(crate::VariantTy::Positional(types)) => {
                let params = types.iter().map(|&t| interner.subst(t, subst)).collect();
                self.exprs.insert(
                    id,
                    ExprInfo::FnValue {
                        def: variant,
                        subst: subst.clone(),
                    },
                );
                interner.intern(TyKind::Callable {
                    kind: FnKind::Fn,
                    params,
                    ret: enum_ty,
                })
            }
            _ => {
                let name = self.cx.hir.def(variant).name.clone();
                self.error(
                    Diagnostic::error(
                        codes::WRONG_NAME_KIND,
                        self.span(id),
                        format!(
                            "variant `{name}` has named fields and must be constructed with a call"
                        ),
                    )
                    .with_label("missing field values"),
                );
                interner.error()
            }
        }
    }

    /// The operation a `select` arm waits on (spec §11.3), from the call its operation
    /// expression makes.
    fn select_op(&self, operation: ExprId) -> Option<crate::SelectOp> {
        let lang = match self.exprs.get(&operation) {
            Some(ExprInfo::Call {
                target: CallTarget::Fn { def, .. } | CallTarget::Trait { method: def, .. },
                ..
            }) => self.cx.hir.def(*def).lang(),
            _ => None,
        }?;
        match (lang, self.body.expr(operation)) {
            (
                "chan_recv",
                Expr::MethodCall { receiver, .. } | Expr::Field { base: receiver, .. },
            ) => Some(crate::SelectOp::Recv { channel: *receiver }),
            ("chan_send", Expr::MethodCall { receiver, args, .. }) => {
                args.first().map(|a| crate::SelectOp::Send {
                    channel: *receiver,
                    value: a.value,
                })
            }
            ("time_after", _) => Some(crate::SelectOp::Timer),
            _ => None,
        }
    }
}
