//! Evaluation of bodies, expressions, statements and patterns.

use crate::value::{Closure, EnumValue, MapValue, Projection, StructValue};
use crate::{Eval, Flow, Interp, MAX_DEPTH, Place, Value, rc_subst};
use loom_check::{
    ArgSource, CallTarget, ExprInfo, ForKind, ImplKind, PatInfo, Selection, Solver, TraitRef,
    TypedBody,
};
use loom_hir::{
    BinaryOp, Body, BodyId, DefId, DefKind, Expr, ExprId, InterpolationPart, Literal, Mode, Pat,
    PatId, Stmt, UnaryOp,
};
use loom_span::Span;
use loom_types::{InferTable, Subst, Ty, TyKind};
use std::rc::Rc;

/// An argument: a value, or a place for a `mut` parameter.
#[derive(Clone, Debug)]
pub enum ArgVal {
    /// A value.
    Value(Value),
    /// A place.
    Place(Place),
}

impl ArgVal {
    /// The value (read from the place if it is one).
    pub fn value(&self) -> Value {
        match self {
            Self::Value(v) => v.clone(),
            Self::Place(p) => p.read(),
        }
    }

    fn into_place(self) -> Place {
        match self {
            Self::Value(v) => Place::new(v),
            Self::Place(p) => p,
        }
    }
}

/// A call frame.
pub(crate) struct Frame<'b> {
    body_id: BodyId,
    body: &'b Body,
    typed: &'b TypedBody,
    slots: Vec<Option<Place>>,
    self_place: Option<Place>,
    subst: Rc<Subst>,
    ret_ty: Ty,
}

/// A trait method resolved for a concrete type.
enum Resolved {
    Fn(DefId, Subst),
    Builtin(String, String),
}

impl Interp<'_> {
    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    #[allow(
        clippy::unnecessary_wraps,
        reason = "mirrors the fallible signatures around it"
    )]
    fn span(&self, frame: &Frame<'_>, expr: ExprId) -> Option<Span> {
        Some(frame.body.expr_span(expr))
    }

    fn ty_of(&self, frame: &Frame<'_>, expr: ExprId) -> Ty {
        self.concrete(frame.typed.expr_ty(expr), &frame.subst)
    }

    fn lang_ty(&self, name: &str, args: Vec<Ty>) -> Ty {
        match self.items().lang.get(name) {
            Some(def) => self.interner().intern(TyKind::Adt(def, args)),
            None => self.interner().error(),
        }
    }

    /// The concrete type of a value, as far as the value knows it.
    pub(crate) fn value_ty(&self, value: &Value) -> Ty {
        let interner = self.interner();
        match value {
            Value::Unit => interner.unit(),
            Value::Bool(_) => interner.bool(),
            Value::Int(_, t) => interner.intern(TyKind::Int(*t)),
            Value::Float(_, t) => interner.intern(TyKind::Float(*t)),
            Value::Char(_) => interner.intern(TyKind::Char),
            Value::Str(_) => interner.string(),
            Value::Bytes(_) => interner.intern(TyKind::Bytes),
            Value::Tuple(elements) => interner.intern(TyKind::Tuple(
                elements.iter().map(|e| self.value_ty(e)).collect(),
            )),
            Value::Struct(s) => s.ty,
            Value::Enum(e) => e.ty,
            Value::Array(_) => self.lang_ty("Array", vec![interner.error()]),
            Value::Map(_) => self.lang_ty("Map", vec![interner.error(), interner.error()]),
            Value::Closure(_) | Value::Function { .. } => interner.error(),
            Value::Handle(h) => self.lang_ty(h.lang(), Vec::new()),
        }
    }

    // ----- calls -----------------------------------------------------------------------------

    /// Calls function `def` with concrete generic arguments `subst`. `args` has one entry
    /// per parameter; `None` means the parameter's default.
    pub(crate) fn call_fn(
        &self,
        def: DefId,
        subst: &Subst,
        receiver: Option<ArgVal>,
        args: Vec<Option<ArgVal>>,
        span: Option<Span>,
    ) -> Eval<Value> {
        let data = self.hir.def(def);
        if let DefKind::Variant(_) = data.kind {
            let values = args
                .into_iter()
                .map(|a| a.map_or(Value::Unit, |a| a.value()))
                .collect();
            let enum_def = data.parent.unwrap_or(def);
            let ty = self.enum_ty_from_subst(enum_def, subst);
            return Ok(self.make_variant(def, ty, values));
        }
        let DefKind::Fn(fn_data) = &data.kind else {
            return Err(Flow::Panic(
                format!("`{}` cannot be called", data.name),
                span,
            ));
        };
        if fn_data.extern_abi.as_deref() == Some("intrinsic") {
            let values: Vec<ArgVal> = args
                .into_iter()
                .map(|a| a.unwrap_or(ArgVal::Value(Value::Unit)))
                .collect();
            return self.intrinsic(def, data.name.as_str(), subst, values, span);
        }
        let Some(body_id) = fn_data.body else {
            return Err(Flow::Panic(format!("`{}` has no body", data.name), span));
        };
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Err(Flow::Panic("stack overflow".to_owned(), span));
        }
        self.depth.set(depth + 1);
        let result = self.run_body(def, body_id, subst, receiver, args);
        self.depth.set(depth);
        result
    }

    #[allow(
        clippy::match_same_arms,
        reason = "arms listed per case for readability"
    )]
    fn run_body(
        &self,
        def: DefId,
        body_id: BodyId,
        subst: &Subst,
        receiver: Option<ArgVal>,
        args: Vec<Option<ArgVal>>,
    ) -> Eval<Value> {
        let body = self.hir.body(body_id);
        let typed = self.typed(body_id);
        let sig = self.items().fns.get(&def);
        let ret_ty = sig.map_or_else(|| self.interner().unit(), |s| self.concrete(s.ret, subst));
        let mut frame = Frame {
            body_id,
            body,
            typed,
            slots: vec![None; body.bindings.len()],
            self_place: receiver.map(ArgVal::into_place),
            subst: rc_subst(subst.clone()),
            ret_ty,
        };
        let mut pending_defaults = Vec::new();
        for (i, arg) in args.into_iter().enumerate() {
            let Some(&binding) = body.params.get(i) else {
                continue;
            };
            match arg {
                Some(arg) => frame.slots[binding.index()] = Some(arg.into_place()),
                None => pending_defaults.push(i),
            }
        }
        for i in pending_defaults {
            let Some(Some(default)) = body.defaults.get(i) else {
                continue;
            };
            let value = self.eval(&mut frame, *default)?;
            frame.slots[body.params[i].index()] = Some(Place::new(value));
        }
        let Some(root) = body.root else {
            return Ok(Value::Unit);
        };
        let value = match self.eval(&mut frame, root) {
            Ok(v) => v,
            Err(Flow::Return(v)) => v,
            Err(other) => return Err(other),
        };
        if matches!(self.interner().kind(frame.ret_ty), TyKind::Unit) {
            Ok(Value::Unit)
        } else {
            Ok(value)
        }
    }

    fn enum_ty_from_subst(&self, enum_def: DefId, subst: &Subst) -> Ty {
        let generics = self.items().generics_of(enum_def);
        let args = generics
            .own
            .iter()
            .map(|&p| subst.get(p).unwrap_or_else(|| self.interner().error()))
            .collect();
        self.interner().intern(TyKind::Adt(enum_def, args))
    }

    pub(crate) fn make_variant(&self, variant: DefId, ty: Ty, fields: Vec<Value>) -> Value {
        let index = match &self.hir.def(variant).kind {
            DefKind::Variant(v) => v.index,
            _ => 0,
        };
        Value::Enum(Rc::new(EnumValue {
            ty,
            variant,
            index,
            fields,
        }))
    }

    /// The variant named `name` of the lang enum `lang`.
    pub(crate) fn lang_variant(
        &self,
        lang: &str,
        name: &str,
        args: Vec<Ty>,
        fields: Vec<Value>,
    ) -> Value {
        let Some(enum_def) = self.items().lang.get(lang) else {
            return Value::Unit;
        };
        let DefKind::Enum(data) = &self.hir.def(enum_def).kind else {
            return Value::Unit;
        };
        let Some(&variant) = data
            .variants
            .iter()
            .find(|&&v| self.hir.def(v).name.as_str() == name)
        else {
            return Value::Unit;
        };
        let ty = self.interner().intern(TyKind::Adt(enum_def, args));
        self.make_variant(variant, ty, fields)
    }

    /// Resolves trait method `method` for the concrete `self_ty`.
    fn resolve_trait_method(
        &self,
        method: DefId,
        self_ty: Ty,
        subst: &Subst,
        receiver: Option<&Value>,
        span: Option<Span>,
    ) -> Eval<Resolved> {
        let interner = self.interner();
        let Some(trait_) = self.hir.def(method).parent else {
            return Err(Flow::Panic("trait method without a trait".to_owned(), span));
        };
        let mut self_ty = self_ty;
        if let (TyKind::Dyn(..) | TyKind::Error, Some(value)) = (interner.kind(self_ty), receiver) {
            self_ty = self.value_ty(value);
        }
        let generics = self.items().generics_of(trait_);
        let trait_ref = TraitRef {
            trait_,
            args: generics
                .own
                .iter()
                .map(|&p| subst.get(p).unwrap_or_else(|| interner.error()))
                .collect(),
        };
        let mut table = InferTable::new();
        let selection = Solver {
            hir: self.hir,
            items: self.items(),
            interner,
            table: &mut table,
        }
        .select(self_ty, &trait_ref, &loom_check::Env::default());
        let name = self.hir.def(method).name.clone();
        let lang = self.hir.def(trait_).lang().unwrap_or("").to_owned();
        let mut full = subst.clone();
        full.insert(Items::self_param(trait_), self_ty);
        match selection {
            Ok(Selection::Impl {
                impl_id,
                subst: impl_subst,
            }) => {
                let info = &self.items().impls[impl_id];
                if let ImplKind::Derived(_) = info.kind {
                    return Ok(Resolved::Builtin(lang, name.to_string()));
                }
                match info.methods.get(&name) {
                    Some(&impl_method) => {
                        let mut s = impl_subst;
                        let decl_own = self.items().generics_of(method).own;
                        let impl_own = self.items().generics_of(impl_method).own;
                        for (&d, &i) in decl_own.iter().zip(&impl_own) {
                            if let Some(t) = subst.get(d) {
                                s.insert(i, t);
                            }
                        }
                        Ok(Resolved::Fn(impl_method, s))
                    }
                    None => Ok(Resolved::Fn(method, full)),
                }
            }
            Ok(Selection::Builtin) => Ok(Resolved::Builtin(lang, name.to_string())),
            other => Err(Flow::Panic(
                format!(
                    "internal error: cannot dispatch `{name}` for `{}` ({other:?})",
                    loom_check::display::ty_to_string(self.hir, self.items(), interner, self_ty)
                ),
                span,
            )),
        }
    }

    /// Calls trait method `name` of lang trait `trait_lang` on a value of type `self_ty`.
    pub(crate) fn call_trait_method(
        &self,
        trait_lang: &str,
        name: &str,
        self_ty: Ty,
        receiver: ArgVal,
        args: Vec<Option<ArgVal>>,
        span: Option<Span>,
    ) -> Eval<Value> {
        let Some(method) = self.method_named(trait_lang, name) else {
            return Err(Flow::Panic(
                format!("missing lang method {trait_lang}.{name}"),
                span,
            ));
        };
        let has_receiver =
            matches!(&self.hir.def(method).kind, DefKind::Fn(f) if f.receiver.is_some());
        let receiver_value = receiver.value();
        let resolved =
            self.resolve_trait_method(method, self_ty, &Subst::new(), Some(&receiver_value), span)?;
        self.call_resolved(
            resolved,
            if has_receiver { Some(receiver) } else { None },
            args,
            span,
        )
    }

    fn call_resolved(
        &self,
        resolved: Resolved,
        receiver: Option<ArgVal>,
        args: Vec<Option<ArgVal>>,
        span: Option<Span>,
    ) -> Eval<Value> {
        match resolved {
            Resolved::Fn(def, subst) => self.call_fn(def, &subst, receiver, args, span),
            Resolved::Builtin(trait_, method) => {
                let this = receiver.map(|r| r.value());
                let other = args.into_iter().next().flatten().map(|a| a.value());
                self.builtin_method(&trait_, &method, this, other, span)
            }
        }
    }

    fn builtin_method(
        &self,
        trait_: &str,
        method: &str,
        this: Option<Value>,
        other: Option<Value>,
        span: Option<Span>,
    ) -> Eval<Value> {
        let this = this.unwrap_or(Value::Unit);
        match (trait_, method) {
            ("Eq", "eq") => Ok(Value::Bool(
                self.value_eq(&this, &other.unwrap_or(Value::Unit))?,
            )),
            ("Eq", "ne") => Ok(Value::Bool(
                !self.value_eq(&this, &other.unwrap_or(Value::Unit))?,
            )),
            ("Ord", "compare") => {
                let ordering = self.value_cmp(&this, &other.unwrap_or(Value::Unit))?;
                Ok(self.ordering_value(ordering))
            }
            ("Clone", "clone") => Ok(this),
            _ => Err(Flow::Panic(
                format!("internal error: no built-in `{trait_}.{method}`"),
                span,
            )),
        }
    }

    /// Calls what `target` designates.
    fn call_target(
        &self,
        frame: &mut Frame<'_>,
        call: ExprId,
        target: &CallTarget,
        receiver: Option<ExprId>,
        sources: &[ArgSource],
        args: &[loom_hir::Arg],
    ) -> Eval<Value> {
        let span = self.span(frame, call);
        let modes = self.param_modes(target);
        let receiver_mode = match target {
            CallTarget::Fn { def, .. } | CallTarget::Trait { method: def, .. } => {
                self.items().fns.get(def).and_then(|s| s.receiver)
            }
            _ => None,
        };
        let receiver_val = match receiver {
            Some(r) if receiver_mode == Some(Mode::Mut) => {
                Some(ArgVal::Place(self.eval_place(frame, r)?))
            }
            Some(r) => Some(ArgVal::Value(self.eval(frame, r)?)),
            None => None,
        };
        // Arguments are evaluated in the order written (spec §7).
        let mut evaluated: Vec<Option<ArgVal>> = vec![None; args.len()];
        let callee_value = if let CallTarget::Value = target {
            let Expr::Call { callee, .. } = frame.body.expr(call) else {
                return Ok(Value::Unit);
            };
            Some(self.eval(frame, *callee)?)
        } else {
            None
        };
        for (i, arg) in args.iter().enumerate() {
            let param = sources.iter().position(|s| *s == ArgSource::Arg(i));
            let mutable = param.is_some_and(|p| modes.get(p) == Some(&Mode::Mut));
            evaluated[i] = Some(if mutable {
                ArgVal::Place(self.eval_place(frame, arg.value)?)
            } else {
                ArgVal::Value(self.eval(frame, arg.value)?)
            });
        }
        let per_param: Vec<Option<ArgVal>> = sources
            .iter()
            .map(|s| match s {
                ArgSource::Arg(i) => evaluated[*i].take(),
                ArgSource::Default => None,
            })
            .collect();
        match target {
            CallTarget::Fn { def, subst } => {
                let subst = self.concrete_subst(subst, &frame.subst);
                self.call_fn(*def, &subst, receiver_val, per_param, span)
            }
            CallTarget::Trait {
                method,
                self_ty,
                subst,
            } => {
                let subst = self.concrete_subst(subst, &frame.subst);
                let self_ty = self.concrete(*self_ty, &frame.subst);
                let receiver_value = receiver_val.as_ref().map(ArgVal::value);
                let resolved = self.resolve_trait_method(
                    *method,
                    self_ty,
                    &subst,
                    receiver_value.as_ref(),
                    span,
                )?;
                self.call_resolved(resolved, receiver_val, per_param, span)
            }
            CallTarget::Struct { def, .. } => {
                let ty = self.ty_of(frame, call);
                let fields = per_param
                    .into_iter()
                    .map(|a| a.map_or(Value::Unit, |a| a.value()))
                    .collect();
                let _ = def;
                Ok(Value::Struct(Rc::new(StructValue { ty, fields })))
            }
            CallTarget::Variant { def, .. } => {
                let ty = self.ty_of(frame, call);
                let fields = per_param
                    .into_iter()
                    .map(|a| a.map_or(Value::Unit, |a| a.value()))
                    .collect();
                Ok(self.make_variant(*def, ty, fields))
            }
            CallTarget::Value => {
                let callee = callee_value.unwrap_or(Value::Unit);
                let values = per_param
                    .into_iter()
                    .map(|a| a.unwrap_or(ArgVal::Value(Value::Unit)))
                    .collect();
                self.call_value(&callee, values, span)
            }
        }
    }

    fn param_modes(&self, target: &CallTarget) -> Vec<Mode> {
        match target {
            CallTarget::Fn { def, .. } | CallTarget::Trait { method: def, .. } => self
                .items()
                .fns
                .get(def)
                .map(|s| s.params.iter().map(|p| p.0).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn concrete_subst(&self, subst: &Subst, frame_subst: &Subst) -> Subst {
        subst.map(|t| self.concrete(t, frame_subst))
    }

    /// Calls a closure or function value.
    pub(crate) fn call_value(
        &self,
        callee: &Value,
        args: Vec<ArgVal>,
        span: Option<Span>,
    ) -> Eval<Value> {
        match callee {
            Value::Closure(closure) => self.call_closure(closure, args, span),
            Value::Function { def, subst } => self.call_fn(
                *def,
                subst,
                None,
                args.into_iter().map(Some).collect(),
                span,
            ),
            _ => Err(Flow::Panic(
                "internal error: calling a non-function".to_owned(),
                span,
            )),
        }
    }

    #[allow(
        clippy::match_same_arms,
        reason = "arms listed per case for readability"
    )]
    fn call_closure(
        &self,
        closure: &Closure,
        args: Vec<ArgVal>,
        span: Option<Span>,
    ) -> Eval<Value> {
        let body = self.hir.body(closure.body);
        let typed = self.typed(closure.body);
        let Expr::Closure {
            params,
            body: closure_body,
        } = body.expr(closure.expr)
        else {
            return Ok(Value::Unit);
        };
        let closure_ty = self.concrete(typed.expr_ty(closure.expr), &closure.subst);
        let ret_ty = match self.interner().kind(closure_ty) {
            TyKind::Callable { ret, .. } => ret,
            _ => self.interner().error(),
        };
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Err(Flow::Panic("stack overflow".to_owned(), span));
        }
        self.depth.set(depth + 1);
        let mut slots = closure.slots.clone();
        slots.resize(body.bindings.len(), None);
        let mut frame = Frame {
            body_id: closure.body,
            body,
            typed,
            slots,
            self_place: closure.self_place.clone(),
            subst: Rc::clone(&closure.subst),
            ret_ty,
        };
        let mut result = Ok(Value::Unit);
        for (param, arg) in params.iter().flatten().zip(args) {
            match (body.pat(param.pat), arg) {
                (&Pat::Bind(b), ArgVal::Place(place)) if param.mode == Mode::Mut => {
                    frame.slots[typed.canonical(b).index()] = Some(place);
                }
                (_, arg) => {
                    let value = arg.value();
                    if let Err(flow) = self.match_pat(&mut frame, param.pat, &value) {
                        result = Err(flow);
                    }
                }
            }
        }
        if result.is_ok() {
            result = match self.eval(&mut frame, *closure_body) {
                Ok(v) => Ok(v),
                Err(Flow::Return(v)) => Ok(v),
                Err(other) => Err(other),
            };
        }
        self.depth.set(depth);
        let value = result?;
        if matches!(self.interner().kind(ret_ty), TyKind::Unit) {
            Ok(Value::Unit)
        } else {
            Ok(value)
        }
    }

    // ----- places ----------------------------------------------------------------------------

    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    fn slot(&self, frame: &Frame<'_>, binding: loom_hir::BindingId) -> Place {
        let binding = frame.typed.canonical(binding);
        frame.slots[binding.index()]
            .clone()
            .unwrap_or_else(|| Place::new(Value::Unit))
    }

    pub(crate) fn eval_place(&self, frame: &mut Frame<'_>, expr: ExprId) -> Eval<Place> {
        match frame.body.expr(expr) {
            Expr::Name(_) => {
                if let Some(ExprInfo::Local(b)) = frame.typed.exprs.get(&expr) {
                    return Ok(self.slot(frame, *b));
                }
            }
            Expr::SelfValue => {
                if let Some(place) = &frame.self_place {
                    return Ok(place.clone());
                }
            }
            &Expr::Field { base, .. } => {
                if let Some(&ExprInfo::Field(i)) = frame.typed.exprs.get(&expr) {
                    let base = self.eval_place(frame, base)?;
                    return Ok(base.project(Projection::Field(i)));
                }
            }
            &Expr::TupleField { base, index } => {
                let base = self.eval_place(frame, base)?;
                return Ok(base.project(Projection::Field(index as usize)));
            }
            Expr::Index { base, indices, .. } => {
                let (base, indices) = (*base, indices.clone());
                let place = self.eval_place(frame, base)?;
                let key = self.eval(frame, indices[0])?;
                let span = self.span(frame, expr);
                return match place.read() {
                    Value::Array(items) => {
                        let index = self.check_index(&key, items.len(), span)?;
                        Ok(place.project(Projection::Index(index)))
                    }
                    Value::Map(map) => match self.map_find(&map, &key)? {
                        Some(i) => Ok(place.project(Projection::MapValue(i))),
                        None => Err(Flow::Panic("key not found in map".to_owned(), span)),
                    },
                    _ => Err(Flow::Panic(
                        "internal error: indexing a non-collection".to_owned(),
                        span,
                    )),
                };
            }
            _ => {}
        }
        Ok(Place::new(self.eval(frame, expr)?))
    }

    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    pub(crate) fn check_index(
        &self,
        key: &Value,
        length: usize,
        span: Option<Span>,
    ) -> Eval<usize> {
        let index = key.as_int();
        if index < 0 || index >= i128::try_from(length).unwrap_or(i128::MAX) {
            return Err(Flow::Panic(
                format!("index out of bounds: the length is {length} but the index is {index}"),
                span,
            ));
        }
        Ok(usize::try_from(index).unwrap_or(0))
    }

    // ----- expressions -----------------------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
    pub(crate) fn eval(&self, frame: &mut Frame<'_>, id: ExprId) -> Eval<Value> {
        let body = frame.body;
        match body.expr(id) {
            Expr::Missing | Expr::Unsupported(_) => Err(Flow::Panic(
                "internal error: evaluating an erroneous expression".to_owned(),
                self.span(frame, id),
            )),
            Expr::Literal(lit) => Ok(self.literal(frame, id, lit, false)),
            Expr::Interpolate(parts) => {
                let mut text = String::new();
                for part in parts {
                    match part {
                        InterpolationPart::Text(t) => text.push_str(t),
                        InterpolationPart::Expr(e) => {
                            let value = self.eval(frame, *e)?;
                            let ty = self.ty_of(frame, *e);
                            text.push_str(&self.display(&value, ty, self.span(frame, *e))?);
                        }
                    }
                }
                Ok(Value::string(text))
            }
            Expr::Name(_) => self.path_value(frame, id),
            Expr::SelfValue => Ok(frame.self_place.as_ref().map_or(Value::Unit, Place::read)),
            Expr::SelfType => Ok(Value::Unit),
            &Expr::Field { base, .. } => match frame.typed.exprs.get(&id).cloned() {
                Some(ExprInfo::Field(i)) => {
                    let value = self.eval(frame, base)?;
                    Ok(match value {
                        Value::Struct(s) => s.fields.get(i).cloned().unwrap_or(Value::Unit),
                        Value::Enum(e) => e.fields.get(i).cloned().unwrap_or(Value::Unit),
                        _ => Value::Unit,
                    })
                }
                Some(ExprInfo::Call {
                    target,
                    receiver,
                    args,
                }) => self.call_target(frame, id, &target, receiver, &args, &[]),
                _ => self.path_value(frame, id),
            },
            &Expr::TupleField { base, index } => match self.eval(frame, base)? {
                Value::Tuple(t) => Ok(t.get(index as usize).cloned().unwrap_or(Value::Unit)),
                _ => Ok(Value::Unit),
            },
            Expr::Call { args, .. } | Expr::MethodCall { args, .. } => {
                match frame.typed.exprs.get(&id).cloned() {
                    Some(ExprInfo::Call {
                        target,
                        receiver,
                        args: sources,
                    }) => self.call_target(frame, id, &target, receiver, &sources, args),
                    Some(
                        ExprInfo::UnitVariant { .. }
                        | ExprInfo::Const { .. }
                        | ExprInfo::FnValue { .. },
                    ) => self.path_value(frame, id),
                    _ => Err(Flow::Panic(
                        "internal error: unresolved call".to_owned(),
                        self.span(frame, id),
                    )),
                }
            }
            Expr::Index { base, indices, .. } => {
                let (base, index_expr) = (*base, indices[0]);
                let container = self.eval(frame, base)?;
                let key = self.eval(frame, index_expr)?;
                let span = self.span(frame, id);
                match container {
                    Value::Array(items) => {
                        let index = self.check_index(&key, items.len(), span)?;
                        Ok(items[index].clone())
                    }
                    Value::Map(map) => match self.map_find(&map, &key)? {
                        Some(i) => Ok(map.entries[i].1.clone()),
                        None => Err(Flow::Panic("key not found in map".to_owned(), span)),
                    },
                    _ => Err(Flow::Panic(
                        "internal error: indexing a non-collection".to_owned(),
                        span,
                    )),
                }
            }
            &Expr::Unary { op, operand } => {
                if op == UnaryOp::Neg
                    && let Expr::Literal(lit @ Literal::Int(_)) = body.expr(operand)
                {
                    return Ok(self.literal(frame, operand, lit, true));
                }
                let value = self.eval(frame, operand)?;
                let span = self.span(frame, id);
                match (op, value) {
                    (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (UnaryOp::Neg, Value::Int(v, t)) => {
                        self.int_result(0i128.checked_sub(v), t, "negate", span)
                    }
                    (UnaryOp::Neg, Value::Float(v, t)) => Ok(Value::Float(-v, t)),
                    (UnaryOp::BitNot, Value::Int(v, t)) => {
                        Ok(Value::Int(crate::ops::wrap(!v, t), t))
                    }
                    _ => Err(Flow::Panic(
                        "internal error: bad unary operand".to_owned(),
                        span,
                    )),
                }
            }
            &Expr::Binary { op, lhs, rhs, .. } => self.binary(frame, id, op, lhs, rhs),
            &Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let start = self.eval(frame, start)?;
                let end = self.eval(frame, end)?;
                let ty = self.ty_of(frame, id);
                Ok(Value::Struct(Rc::new(StructValue {
                    ty,
                    fields: vec![start, end, Value::Bool(inclusive)],
                })))
            }
            &Expr::In { expr, pat } => {
                let value = self.eval(frame, expr)?;
                Ok(Value::Bool(self.match_pat(frame, pat, &value)?))
            }
            &Expr::Try(inner) => {
                let value = self.eval(frame, inner)?;
                let Value::Enum(e) = &value else {
                    return Ok(Value::Unit);
                };
                let name = self.hir.def(e.variant).name.as_str().to_owned();
                match name.as_str() {
                    "Ok" | "Some" => Ok(e.fields.first().cloned().unwrap_or(Value::Unit)),
                    "Err" => {
                        let error = e.fields.first().cloned().unwrap_or(Value::Unit);
                        let error = match frame.typed.exprs.get(&id).cloned() {
                            Some(ExprInfo::Try {
                                convert: Some(target),
                                ..
                            }) => {
                                let span = self.span(frame, id);
                                match target {
                                    CallTarget::Trait {
                                        method,
                                        self_ty,
                                        subst,
                                    } => {
                                        let subst = self.concrete_subst(&subst, &frame.subst);
                                        let self_ty = self.concrete(self_ty, &frame.subst);
                                        let resolved = self.resolve_trait_method(
                                            method, self_ty, &subst, None, span,
                                        )?;
                                        self.call_resolved(
                                            resolved,
                                            None,
                                            vec![Some(ArgVal::Value(error))],
                                            span,
                                        )?
                                    }
                                    _ => error,
                                }
                            }
                            _ => error,
                        };
                        let ret = frame.ret_ty;
                        let args = match self.interner().kind(ret) {
                            TyKind::Adt(_, args) => args,
                            _ => Vec::new(),
                        };
                        Err(Flow::Return(self.lang_variant(
                            "Result",
                            "Err",
                            args,
                            vec![error],
                        )))
                    }
                    "None" => {
                        let args = match self.interner().kind(frame.ret_ty) {
                            TyKind::Adt(_, args) => args,
                            _ => Vec::new(),
                        };
                        Err(Flow::Return(self.lang_variant(
                            "Option",
                            "None",
                            args,
                            Vec::new(),
                        )))
                    }
                    _ => Ok(value),
                }
            }
            Expr::Tuple(elements) => {
                if elements.is_empty() {
                    return Ok(Value::Unit);
                }
                let mut values = Vec::with_capacity(elements.len());
                for &e in elements {
                    values.push(self.eval(frame, e)?);
                }
                Ok(Value::Tuple(Rc::new(values)))
            }
            Expr::Array(elements) => {
                let mut values = Vec::with_capacity(elements.len());
                for &e in elements {
                    values.push(self.eval(frame, e)?);
                }
                Ok(Value::Array(Rc::new(values)))
            }
            Expr::Map(entries) => {
                let mut map = MapValue::default();
                for &(k, v) in entries {
                    let key = self.eval(frame, k)?;
                    let value = self.eval(frame, v)?;
                    self.map_insert(&mut map, key, value)?;
                }
                Ok(Value::Map(Rc::new(map)))
            }
            Expr::Closure { .. } => {
                let (slots, self_place) = if self.escaping.contains(&(frame.body_id, id)) {
                    // Captures by copy (spec §8.4 rule 1).
                    let copy = |p: &Place| Place::new(p.read());
                    (
                        frame.slots.iter().map(|s| s.as_ref().map(copy)).collect(),
                        frame.self_place.as_ref().map(copy),
                    )
                } else {
                    (frame.slots.clone(), frame.self_place.clone())
                };
                Ok(Value::Closure(Rc::new(Closure {
                    body: frame.body_id,
                    expr: id,
                    slots,
                    self_place,
                    subst: Rc::clone(&frame.subst),
                })))
            }
            Expr::Block { stmts } => {
                let mut last = Value::Unit;
                let count = stmts.len();
                for (i, stmt) in stmts.iter().enumerate() {
                    let value = self.stmt(frame, stmt)?;
                    if i + 1 == count {
                        last = value;
                    }
                }
                Ok(last)
            }
            &Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                if self.eval(frame, cond)?.as_bool() {
                    let v = self.eval(frame, then_branch)?;
                    Ok(if else_branch.is_some() {
                        v
                    } else {
                        Value::Unit
                    })
                } else if let Some(e) = else_branch {
                    self.eval(frame, e)
                } else {
                    Ok(Value::Unit)
                }
            }
            Expr::Case {
                scrutinee,
                arms,
                else_branch,
            } => {
                let value = self.eval(frame, *scrutinee)?;
                for arm in arms {
                    if self.match_pat(frame, arm.pat, &value)? {
                        if let Some(guard) = arm.guard
                            && !self.eval(frame, guard)?.as_bool()
                        {
                            continue;
                        }
                        return self.eval(frame, arm.body);
                    }
                }
                match else_branch {
                    Some(e) => self.eval(frame, *e),
                    None => Err(Flow::Panic(
                        "no `case` arm matched".to_owned(),
                        self.span(frame, id),
                    )),
                }
            }
            &Expr::Loop(loop_body) => loop {
                match self.eval(frame, loop_body) {
                    Ok(_) | Err(Flow::Next) => {}
                    Err(Flow::Break(v)) => return Ok(v),
                    Err(other) => return Err(other),
                }
            },
            &Expr::While {
                cond,
                body: loop_body,
            } => {
                while self.eval(frame, cond)?.as_bool() {
                    match self.eval(frame, loop_body) {
                        Ok(_) | Err(Flow::Next) => {}
                        Err(Flow::Break(_)) => break,
                        Err(other) => return Err(other),
                    }
                }
                Ok(Value::Unit)
            }
            &Expr::For {
                pat,
                iter,
                body: loop_body,
            } => self.for_loop(frame, id, pat, iter, loop_body),
            &Expr::Return(value) => {
                let v = match value {
                    Some(v) => self.eval(frame, v)?,
                    None => Value::Unit,
                };
                Err(Flow::Return(v))
            }
            &Expr::Break(value) => {
                let v = match value {
                    Some(v) => self.eval(frame, v)?,
                    None => Value::Unit,
                };
                Err(Flow::Break(v))
            }
            Expr::Next => Err(Flow::Next),
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "F32 values are stored as F64"
    )]
    fn literal(&self, frame: &Frame<'_>, id: ExprId, lit: &Literal, negative: bool) -> Value {
        match lit {
            Literal::Int(v) => {
                let ty = self.ty_of(frame, id);
                let magnitude = i128::try_from(*v).unwrap_or_else(|_| v.cast_signed());
                match self.interner().kind(ty) {
                    TyKind::Int(t) => Value::Int(
                        if negative {
                            magnitude.wrapping_neg()
                        } else {
                            magnitude
                        },
                        t,
                    ),
                    TyKind::Float(t) => {
                        #[allow(clippy::cast_precision_loss, reason = "literal conversion")]
                        let f = *v as f64;
                        Value::Float(if negative { -f } else { f }, t)
                    }
                    _ => Value::int(if negative { -magnitude } else { magnitude }),
                }
            }
            Literal::Float(v) => {
                let ty = self.ty_of(frame, id);
                let t = match self.interner().kind(ty) {
                    TyKind::Float(t) => t,
                    _ => loom_types::FloatTy::F64,
                };
                let value = if t == loom_types::FloatTy::F32 {
                    f64::from(*v as f32)
                } else {
                    *v
                };
                Value::Float(if negative { -value } else { value }, t)
            }
            Literal::Char(c) => Value::Char(*c),
            Literal::String(s) => Value::string(s.to_string()),
            Literal::Bytes(b) => Value::Bytes(Rc::new(b.to_vec())),
            Literal::Bool(b) => Value::Bool(*b),
        }
    }

    fn path_value(&self, frame: &mut Frame<'_>, id: ExprId) -> Eval<Value> {
        match frame.typed.exprs.get(&id).cloned() {
            Some(ExprInfo::Local(b)) => Ok(self.slot(frame, b).read()),
            Some(ExprInfo::FnValue { def, subst }) => Ok(Value::Function {
                def,
                subst: rc_subst(self.concrete_subst(&subst, &frame.subst)),
            }),
            Some(ExprInfo::Const { def }) => self.const_value(def),
            Some(ExprInfo::UnitVariant { def }) => {
                let ty = self.ty_of(frame, id);
                Ok(self.make_variant(def, ty, Vec::new()))
            }
            Some(ExprInfo::Call {
                target,
                receiver,
                args,
            }) => self.call_target(frame, id, &target, receiver, &args, &[]),
            _ => Err(Flow::Panic(
                "internal error: unresolved name".to_owned(),
                self.span(frame, id),
            )),
        }
    }

    fn const_value(&self, def: DefId) -> Eval<Value> {
        if let Some(v) = self.consts.borrow().get(&def) {
            return Ok(v.clone());
        }
        let DefKind::Const(c) = &self.hir.def(def).kind else {
            return Ok(Value::Unit);
        };
        let Some(body_id) = c.body else {
            return Ok(Value::Unit);
        };
        let body = self.hir.body(body_id);
        let typed = self.typed(body_id);
        let ret_ty = self
            .items()
            .consts
            .get(&def)
            .copied()
            .unwrap_or_else(|| self.interner().error());
        let mut frame = Frame {
            body_id,
            body,
            typed,
            slots: vec![None; body.bindings.len()],
            self_place: None,
            subst: rc_subst(Subst::new()),
            ret_ty,
        };
        let value = match body.root {
            Some(root) => self.eval(&mut frame, root)?,
            None => Value::Unit,
        };
        self.consts.borrow_mut().insert(def, value.clone());
        Ok(value)
    }

    fn binary(
        &self,
        frame: &mut Frame<'_>,
        id: ExprId,
        op: BinaryOp,
        lhs: ExprId,
        rhs: ExprId,
    ) -> Eval<Value> {
        let span = self.span(frame, id);
        match op {
            BinaryOp::And => {
                if !self.eval(frame, lhs)?.as_bool() {
                    return Ok(Value::Bool(false));
                }
                return Ok(Value::Bool(self.eval(frame, rhs)?.as_bool()));
            }
            BinaryOp::Or => {
                if self.eval(frame, lhs)?.as_bool() {
                    return Ok(Value::Bool(true));
                }
                return Ok(Value::Bool(self.eval(frame, rhs)?.as_bool()));
            }
            _ => {}
        }
        let a = self.eval(frame, lhs)?;
        let b = self.eval(frame, rhs)?;
        if op.is_equality() {
            let equal = self.value_eq(&a, &b)?;
            return Ok(Value::Bool(if op == BinaryOp::Eq { equal } else { !equal }));
        }
        if op.is_ordering() {
            if let (Value::Float(x, _), Value::Float(y, _)) = (&a, &b) {
                return Ok(Value::Bool(match op {
                    BinaryOp::Lt => x < y,
                    BinaryOp::Le => x <= y,
                    BinaryOp::Gt => x > y,
                    _ => x >= y,
                }));
            }
            let ordering = self.value_cmp(&a, &b)?;
            return Ok(Value::Bool(match op {
                BinaryOp::Lt => ordering.is_lt(),
                BinaryOp::Le => ordering.is_le(),
                BinaryOp::Gt => ordering.is_gt(),
                _ => ordering.is_ge(),
            }));
        }
        self.arith(op, &a, &b, span)
    }

    fn for_loop(
        &self,
        frame: &mut Frame<'_>,
        id: ExprId,
        pat: PatId,
        iter: ExprId,
        loop_body: ExprId,
    ) -> Eval<Value> {
        let span = self.span(frame, id);
        let value = self.eval(frame, iter)?;
        let mut iter_ty = self.ty_of(frame, iter);
        let iterator = match frame.typed.exprs.get(&id) {
            Some(ExprInfo::For(ForKind::Iterable)) => {
                let it = self.call_trait_method(
                    "Iterable",
                    "iter",
                    iter_ty,
                    ArgVal::Value(value),
                    Vec::new(),
                    span,
                )?;
                iter_ty = self.value_ty(&it);
                it
            }
            _ => value,
        };
        let place = Place::new(iterator);
        loop {
            let next = self.call_trait_method(
                "Iterator",
                "next_item",
                iter_ty,
                ArgVal::Place(place.clone()),
                Vec::new(),
                span,
            )?;
            let Value::Enum(e) = next else { break };
            if self.hir.def(e.variant).name.as_str() != "Some" {
                break;
            }
            let item = e.fields.first().cloned().unwrap_or(Value::Unit);
            self.match_pat(frame, pat, &item)?;
            match self.eval(frame, loop_body) {
                Ok(_) | Err(Flow::Next) => {}
                Err(Flow::Break(_)) => break,
                Err(other) => return Err(other),
            }
        }
        Ok(Value::Unit)
    }

    // ----- statements ------------------------------------------------------------------------

    fn stmt(&self, frame: &mut Frame<'_>, stmt: &Stmt) -> Eval<Value> {
        match *stmt {
            Stmt::Bind { binding, value, .. } => {
                let v = self.eval(frame, value)?;
                if let Some(&existing) = frame.typed.assignments.get(&binding) {
                    self.slot(frame, existing).write(v);
                } else {
                    frame.slots[frame.typed.canonical(binding).index()] = Some(Place::new(v));
                }
                Ok(Value::Unit)
            }
            Stmt::Assign {
                place, op, value, ..
            } => {
                // `map[key] = value` inserts (spec §7.4 rule 5).
                if op.is_none()
                    && let Expr::Index { base, indices, .. } = frame.body.expr(place)
                    && let TyKind::Adt(def, _) = self.interner().kind(self.ty_of(frame, *base))
                    && Some(def) == self.items().lang.get("Map")
                {
                    let (base, key_expr) = (*base, indices[0]);
                    let target = self.eval_place(frame, base)?;
                    let key = self.eval(frame, key_expr)?;
                    let v = self.eval(frame, value)?;
                    let mut result = Ok(());
                    target.modify(|m| {
                        if let Value::Map(map) = m {
                            result = self.map_insert(Rc::make_mut(map), key, v).map(|_| ());
                        }
                    });
                    result?;
                    return Ok(Value::Unit);
                }
                let target = self.eval_place(frame, place)?;
                let v = self.eval(frame, value)?;
                let new = match op {
                    None => v,
                    Some(op) => {
                        let current = target.read();
                        let span = self.span(frame, place);
                        self.arith(op, &current, &v, span)?
                    }
                };
                target.write(new);
                Ok(Value::Unit)
            }
            Stmt::Expr(e) => self.eval(frame, e),
        }
    }

    // ----- patterns --------------------------------------------------------------------------

    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    fn bind(&self, frame: &mut Frame<'_>, binding: loom_hir::BindingId, value: Value) {
        let canonical = frame.typed.canonical(binding);
        frame.slots[canonical.index()] = Some(Place::new(value));
    }

    /// Whether `value` matches `pat`, binding names on success.
    pub(crate) fn match_pat(&self, frame: &mut Frame<'_>, pat: PatId, value: &Value) -> Eval<bool> {
        let body = frame.body;
        match body.pat(pat) {
            Pat::Missing | Pat::Wild | Pat::Rest => Ok(true),
            &Pat::Bind(b) => {
                self.bind(frame, b, value.clone());
                Ok(true)
            }
            Pat::Literal { lit, negative } => {
                Ok(crate::ops::literal_matches(value, lit, *negative))
            }
            Pat::Range { lo, hi, inclusive } => {
                Ok(crate::ops::range_matches(value, lo, hi, *inclusive))
            }
            Pat::Tuple(elements) => {
                let Value::Tuple(values) = value else {
                    return Ok(matches!(value, Value::Unit) && elements.is_empty());
                };
                let rest = elements
                    .iter()
                    .position(|&e| matches!(body.pat(e), Pat::Rest));
                match rest {
                    None => {
                        for (&e, v) in elements.iter().zip(values.iter()) {
                            if !self.match_pat(frame, e, v)? {
                                return Ok(false);
                            }
                        }
                    }
                    Some(r) => {
                        let after = elements.len() - r - 1;
                        for (i, &e) in elements[..r].iter().enumerate() {
                            if !self.match_pat(frame, e, &values[i])? {
                                return Ok(false);
                            }
                        }
                        for (i, &e) in elements[r + 1..].iter().enumerate() {
                            let index = values.len() - after + i;
                            if !self.match_pat(frame, e, &values[index])? {
                                return Ok(false);
                            }
                        }
                    }
                }
                Ok(true)
            }
            Pat::Path(_) | Pat::TupleStruct { .. } | Pat::Struct { .. } => {
                self.match_path_pat(frame, pat, value)
            }
            Pat::Or(alternatives) => {
                for &a in alternatives {
                    if self.match_pat(frame, a, value)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }

    #[allow(clippy::similar_names, reason = "names mirror the spec terms")]
    fn match_path_pat(&self, frame: &mut Frame<'_>, pat: PatId, value: &Value) -> Eval<bool> {
        let body = frame.body;
        let fields: &[Value] = match (frame.typed.pats.get(&pat).cloned(), value) {
            (Some(PatInfo::Variant(def)), Value::Enum(e)) => {
                if e.variant != def {
                    return Ok(false);
                }
                &e.fields
            }
            (Some(PatInfo::Struct(_)), Value::Struct(s)) => &s.fields,
            (Some(PatInfo::Const(def)), _) => {
                let constant = self.const_value(def)?;
                return self.value_eq(value, &constant);
            }
            _ => return Ok(false),
        };
        let fields = fields.to_vec();
        match body.pat(pat) {
            Pat::TupleStruct { pats, .. } => {
                for (&p, v) in pats.iter().zip(&fields) {
                    if !self.match_pat(frame, p, v)? {
                        return Ok(false);
                    }
                }
            }
            Pat::Struct { fields: named, .. } => {
                let names = self.field_names_of(frame.typed.pats.get(&pat));
                for (name, _, p) in named {
                    let Some(i) = names.iter().position(|n| n == name) else {
                        continue;
                    };
                    if !self.match_pat(frame, *p, &fields[i])? {
                        return Ok(false);
                    }
                }
            }
            _ => {}
        }
        Ok(true)
    }

    fn field_names_of(&self, info: Option<&PatInfo>) -> Vec<loom_hir::Name> {
        match info {
            Some(PatInfo::Variant(def)) => match self.items().variants.get(def) {
                Some(loom_check::VariantTy::Named(fields)) => {
                    fields.iter().map(|f| f.name.clone()).collect()
                }
                _ => Vec::new(),
            },
            Some(PatInfo::Struct(def)) => self
                .items()
                .fields
                .get(def)
                .map(|fs| fs.iter().map(|f| f.name.clone()).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The `Display` text of `value` of type `ty` (spec §7.12).
    pub(crate) fn display(&self, value: &Value, ty: Ty, span: Option<Span>) -> Eval<String> {
        match value {
            Value::Str(s) => return Ok(s.to_string()),
            Value::Int(..) | Value::Float(..) | Value::Bool(_) | Value::Char(_) | Value::Unit => {
                return Ok(crate::ops::primitive_text(value));
            }
            _ => {}
        }
        let result = self.call_trait_method(
            "Display",
            "to_string",
            ty,
            ArgVal::Value(value.clone()),
            Vec::new(),
            span,
        )?;
        Ok(result.as_str().to_owned())
    }
}

use loom_check::Items;
