//! Building MIR from HIR and the checker's `TypedBody` (ADR 0020 §2).

use crate::{
    AggregateKind, BasicBlock, BlockId, Callee, Capture, Constant, Local, LocalDecl, LocalKind,
    Mir, MirBody, MirSource, Operand, Place, Proj, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use loom_check::{
    ArgSource, CallTarget, Checked, Coercion, Env, ExprInfo, ForKind, Items, PatInfo, Solver,
    TraitRef, TypedBody,
};
use loom_hir::{
    BinaryOp, BindingId, Body, BodyId, DefId, DefKind, Expr, ExprId, Hir, InterpolationPart,
    Literal, Mode, Name, Pat, PatId, Stmt, UnaryOp,
};
use loom_span::Span;
use loom_types::{InferTable, Subst, Ty, TyKind};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Builds MIR for every body of the program.
#[must_use]
pub fn build_program(hir: &Hir, checked: &Checked) -> Mir {
    let cx = Cx { hir, checked };
    let mut mir = Mir::default();
    let mut queue: Vec<Job> = Vec::new();
    for (index, body) in hir.bodies.iter().enumerate() {
        let id = BodyId(u32::try_from(index).unwrap_or(u32::MAX));
        match &hir.def(body.owner).kind {
            DefKind::Fn(f) => {
                if f.extern_abi.is_some() {
                    continue;
                }
                if body.root.is_some() {
                    queue.push(Job::Item(id));
                }
                for (i, default) in body.defaults.iter().enumerate() {
                    if default.is_some() {
                        queue.push(Job::Default(id, u32::try_from(i).unwrap_or(0)));
                    }
                }
            }
            DefKind::Const(_) => queue.push(Job::Item(id)),
            _ => {}
        }
    }
    while let Some(job) = queue.pop() {
        let (body, closures) = Builder::build(&cx, &job);
        let source = body.source;
        mir.by_source.insert(source, mir.bodies.len());
        mir.bodies.push(body);
        queue.extend(closures);
    }
    mir
}

struct Cx<'a> {
    hir: &'a Hir,
    checked: &'a Checked,
}

/// A body to build.
enum Job {
    Item(BodyId),
    Default(BodyId, u32),
    Closure {
        body: BodyId,
        expr: ExprId,
        captures: Vec<Capture>,
        escaping: bool,
        /// The enclosing body's binding modes (bindings that are borrows).
        outer_by_ref: HashSet<BindingId>,
    },
}

struct Scope {
    /// Locals to drop at the end of the scope, in declaration order.
    drops: Vec<Local>,
    /// Locals to mark dead at the end of the scope.
    locals: Vec<Local>,
}

struct LoopScope {
    break_block: BlockId,
    continue_block: BlockId,
    result: Option<Place>,
    scope_depth: usize,
    /// The number of statement-temporary levels outside the loop.
    temps_depth: usize,
}

struct Builder<'a> {
    cx: &'a Cx<'a>,
    body_id: BodyId,
    body: &'a Body,
    typed: &'a loom_check::TypedBody,
    mir: MirBody,
    current: BlockId,
    /// Binding → its place (a local, a deref of a borrowed local, or a capture).
    bindings: HashMap<BindingId, Place>,
    /// Bindings whose place is reached through a borrow.
    by_ref: HashSet<BindingId>,
    self_place: Option<Place>,
    scopes: Vec<Scope>,
    loops: Vec<LoopScope>,
    /// Bindings used in an owning position somewhere in the body.
    owning_uses: HashSet<BindingId>,
    /// Closure expressions in owning positions (escaping, spec §8.4).
    escaping_closures: HashSet<ExprId>,
    /// Closures found while building, to build next.
    closures: Vec<Job>,
    env: Env,
    /// Temporaries of the current statement that need dropping at its end.
    stmt_temps: Vec<Vec<Local>>,
}

impl<'a> Builder<'a> {
    fn items(&self) -> &'a Items {
        &self.cx.checked.items
    }

    fn interner(&self) -> &'a loom_types::Interner {
        &self.cx.checked.interner
    }

    #[allow(clippy::too_many_lines, reason = "one setup per kind of body")]
    fn build(cx: &'a Cx<'a>, job: &Job) -> (MirBody, Vec<Job>) {
        let body_id = match job {
            Job::Item(b) | Job::Default(b, _) | Job::Closure { body: b, .. } => *b,
        };
        let body = cx.hir.body(body_id);
        let typed = &cx.checked.bodies[&body_id];
        let owner = body.owner;
        let items = &cx.checked.items;
        let interner = &cx.checked.interner;
        let generics = items.generics_of(owner);
        let mut table = InferTable::new();
        let env = Solver {
            hir: cx.hir,
            items,
            interner,
            table: &mut table,
        }
        .elaborate(&generics.predicates);
        let source = match job {
            Job::Item(b) => MirSource::Item(*b),
            Job::Default(b, i) => MirSource::Default(*b, *i),
            Job::Closure { body, expr, .. } => MirSource::Closure(*body, *expr),
        };
        let span = cx.hir.def(owner).span;
        let mut builder = Builder {
            cx,
            body_id,
            body,
            typed,
            mir: MirBody {
                source,
                owner,
                locals: Vec::new(),
                arg_count: 0,
                blocks: Vec::new(),
                captures: Vec::new(),
                escaping: false,
                span,
            },
            current: BlockId(0),
            bindings: HashMap::new(),
            by_ref: HashSet::new(),
            self_place: None,
            scopes: vec![Scope {
                drops: Vec::new(),
                locals: Vec::new(),
            }],
            loops: Vec::new(),
            owning_uses: HashSet::new(),
            escaping_closures: HashSet::new(),
            closures: Vec::new(),
            env,
            stmt_temps: vec![Vec::new()],
        };
        builder.current = builder.new_block();
        builder.analyze_owning();
        let sig = items.fns.get(&owner);
        let mode_ty = |mode: Mode, ty: Ty| match mode {
            Mode::Borrow => interner.intern(TyKind::Ref {
                mutable: false,
                inner: ty,
            }),
            Mode::Mut => interner.intern(TyKind::Ref {
                mutable: true,
                inner: ty,
            }),
            Mode::Own => ty,
        };
        match job {
            Job::Item(_) | Job::Default(..) => {
                let ret = match (job, sig) {
                    (Job::Default(_, i), Some(sig)) => sig.params[*i as usize].2,
                    (_, Some(sig)) => sig.ret,
                    (_, None) => items
                        .consts
                        .get(&owner)
                        .copied()
                        .unwrap_or_else(|| interner.error()),
                };
                builder.new_local(ret, LocalKind::Return, true, span);
                if let Some(sig) = sig {
                    if let (Some(mode), Some(self_ty)) = (sig.receiver, sig.self_ty) {
                        let mode = if matches!(job, Job::Default(..)) {
                            Mode::Borrow
                        } else {
                            mode
                        };
                        let local = builder.new_local(
                            mode_ty(mode, self_ty),
                            LocalKind::SelfArg,
                            mode == Mode::Own,
                            span,
                        );
                        let place = Place::local(local);
                        builder.self_place = Some(if mode == Mode::Own {
                            place
                        } else {
                            place.project(Proj::Deref)
                        });
                    }
                    let count = match job {
                        Job::Default(_, i) => *i as usize,
                        _ => sig.params.len(),
                    };
                    for (i, (mode, _, ty, _)) in sig.params.iter().enumerate().take(count) {
                        let mode = if matches!(job, Job::Default(..)) {
                            Mode::Borrow
                        } else {
                            *mode
                        };
                        let binding = body.params[i];
                        let local = builder.new_local(
                            mode_ty(mode, *ty),
                            LocalKind::Arg(u32::try_from(i).unwrap_or(0)),
                            false,
                            Span::new(body.file, body.binding(binding).range),
                        );
                        let mut place = Place::local(local);
                        if mode == Mode::Own {
                            builder.mir.locals[local.index()].mutable =
                                body.binding(binding).kind.is_mutable();
                        } else {
                            place = place.project(Proj::Deref);
                            builder.by_ref.insert(binding);
                        }
                        builder.bindings.insert(binding, place);
                    }
                }
                builder.mir.arg_count = builder.mir.locals.len() - 1;
                let root = match job {
                    Job::Default(_, i) => body.defaults[*i as usize],
                    _ => body.root,
                };
                if let Some(root) = root {
                    let ret_place = Place::local(Local(0));
                    builder.expr_into_owning(&ret_place, root);
                }
                builder.exit_all_scopes();
                builder.terminate(TerminatorKind::Return, span);
            }
            Job::Closure {
                expr,
                captures,
                escaping,
                outer_by_ref,
                ..
            } => {
                let _ = outer_by_ref;
                let closure_ty = typed.expr_ty(*expr);
                let (params, ret) = match interner.kind(closure_ty) {
                    TyKind::Callable { params, ret, .. } => (params, ret),
                    _ => (Vec::new(), interner.error()),
                };
                builder.mir.captures.clone_from(captures);
                builder.mir.escaping = *escaping;
                builder.new_local(ret, LocalKind::Return, true, span);
                let mutates = captures.iter().any(|c| c.by_ref == Some(true));
                let env_ty = interner.intern(TyKind::Ref {
                    mutable: mutates || *escaping,
                    inner: closure_ty,
                });
                let env = builder.new_local(env_ty, LocalKind::ClosureEnv, false, span);
                for (i, capture) in captures.iter().enumerate() {
                    let mut place = Place::local(env)
                        .project(Proj::Deref)
                        .project(Proj::Field(u32::try_from(i).unwrap_or(0)));
                    if capture.by_ref.is_some() {
                        place = place.project(Proj::Deref);
                        builder.by_ref.insert(capture.binding);
                    }
                    builder.bindings.insert(capture.binding, place);
                }
                let Expr::Closure {
                    params: closure_params,
                    body: closure_body,
                } = body.expr(*expr)
                else {
                    unreachable!("a closure job points at a closure expression")
                };
                let mut pattern_params = Vec::new();
                for (i, param) in closure_params.iter().flatten().enumerate() {
                    let ty = params.get(i).copied().unwrap_or_else(|| interner.error());
                    let mutable = param.mode == Mode::Mut;
                    let local = builder.new_local(
                        if param.mode == Mode::Own {
                            ty
                        } else {
                            interner.intern(TyKind::Ref { mutable, inner: ty })
                        },
                        LocalKind::Arg(u32::try_from(i).unwrap_or(0)),
                        false,
                        Span::new(body.file, body.pat_range(param.pat)),
                    );
                    let place = if param.mode == Mode::Own {
                        Place::local(local)
                    } else {
                        Place::local(local).project(Proj::Deref)
                    };
                    pattern_params.push((param.pat, place, param.mode != Mode::Own));
                }
                builder.mir.arg_count = builder.mir.locals.len() - 1;
                for (pat, place, borrowed) in pattern_params {
                    if let &Pat::Bind(b) = body.pat(pat) {
                        let b = typed.canonical(b);
                        if borrowed {
                            builder.by_ref.insert(b);
                        }
                        builder.bindings.insert(b, place);
                    } else {
                        let fail = builder.new_block();
                        builder.test_pattern(pat, &place, fail);
                        builder.bind_pattern(pat, &place, borrowed);
                        let ok = builder.current;
                        builder.current = fail;
                        builder.terminate(TerminatorKind::Unreachable, span);
                        builder.current = ok;
                    }
                }
                let ret_place = Place::local(Local(0));
                builder.expr_into_owning(&ret_place, *closure_body);
                builder.exit_all_scopes();
                builder.terminate(TerminatorKind::Return, span);
            }
        }
        builder.seal();
        let closures = std::mem::take(&mut builder.closures);
        (builder.mir, closures)
    }

    /// Gives every block without a terminator an `Unreachable` one.
    fn seal(&mut self) {
        let span = self.mir.span;
        for block in &mut self.mir.blocks {
            if block.terminator.is_none() {
                block.terminator = Some(Terminator {
                    kind: TerminatorKind::Unreachable,
                    span,
                });
            }
        }
    }

    // ----- infrastructure ----------------------------------------------------------------

    fn new_block(&mut self) -> BlockId {
        let id = BlockId(u32::try_from(self.mir.blocks.len()).unwrap_or(u32::MAX));
        self.mir.blocks.push(BasicBlock {
            statements: Vec::new(),
            terminator: None,
        });
        id
    }

    fn new_local(&mut self, ty: Ty, kind: LocalKind, mutable: bool, span: Span) -> Local {
        let id = Local(u32::try_from(self.mir.locals.len()).unwrap_or(u32::MAX));
        self.mir.locals.push(LocalDecl {
            ty,
            kind,
            mutable,
            span,
        });
        id
    }

    fn span(&self, expr: ExprId) -> Span {
        self.body.expr_span(expr)
    }

    fn push(&mut self, kind: StatementKind, span: Span) {
        let block = &mut self.mir.blocks[self.current.index()];
        if block.terminator.is_none() {
            block.statements.push(Statement { kind, span });
        }
    }

    fn assign(&mut self, place: &Place, rvalue: Rvalue, span: Span) {
        self.push(StatementKind::Assign(place.clone(), rvalue), span);
    }

    fn terminate(&mut self, kind: TerminatorKind, span: Span) {
        let block = &mut self.mir.blocks[self.current.index()];
        if block.terminator.is_none() {
            block.terminator = Some(Terminator { kind, span });
        }
    }

    fn goto(&mut self, target: BlockId, span: Span) {
        self.terminate(TerminatorKind::Goto(target), span);
    }

    /// A temporary of type `ty`, dropped at the end of the current statement if needed.
    fn temp(&mut self, ty: Ty, span: Span) -> Local {
        let local = self.new_local(ty, LocalKind::Temp, true, span);
        self.push(StatementKind::StorageLive(local), span);
        if self.needs_drop(ty)
            && let Some(temps) = self.stmt_temps.last_mut()
        {
            temps.push(local);
        }
        local
    }

    fn is_copy(&self, ty: Ty) -> bool {
        let interner = self.interner();
        match interner.kind(ty) {
            TyKind::Ref { mutable, .. } => !mutable,
            TyKind::Int(_)
            | TyKind::Float(_)
            | TyKind::Bool
            | TyKind::Char
            | TyKind::Unit
            | TyKind::Never
            | TyKind::Error => true,
            TyKind::String | TyKind::Bytes | TyKind::Callable { .. } | TyKind::Dyn(..) => false,
            _ => {
                let Some(copy) = self.items().lang.get("Copy") else {
                    return false;
                };
                let mut table = InferTable::new();
                Solver {
                    hir: self.cx.hir,
                    items: self.items(),
                    interner,
                    table: &mut table,
                }
                .select(
                    ty,
                    &TraitRef {
                        trait_: copy,
                        args: Vec::new(),
                    },
                    &self.env,
                )
                .is_ok()
            }
        }
    }

    fn needs_drop(&self, ty: Ty) -> bool {
        !self.is_copy(ty)
    }

    fn unit_operand(&self) -> Operand {
        Operand::Const(Constant::Unit, self.interner().unit())
    }

    fn ty(&self, expr: ExprId) -> Ty {
        self.typed.expr_ty(expr)
    }

    /// The type of `expr` after its coercion.
    fn coerced_ty(&self, expr: ExprId) -> Ty {
        let ty = self.ty(expr);
        match self.typed.coercions.get(&expr) {
            Some(Coercion::Deref) => self.interner().strip_refs(ty),
            Some(Coercion::Autoref) => self.interner().intern(TyKind::Ref {
                mutable: false,
                inner: ty,
            }),
            Some(Coercion::Unsize(target)) => *target,
            None => ty,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope {
            drops: Vec::new(),
            locals: Vec::new(),
        });
    }

    fn pop_scope(&mut self, span: Span) {
        if let Some(scope) = self.scopes.pop() {
            self.emit_scope_exit(&scope, span);
        }
    }

    fn emit_scope_exit(&mut self, scope: &Scope, span: Span) {
        for &local in scope.drops.iter().rev() {
            self.drop_place(Place::local(local), span);
        }
        for &local in scope.locals.iter().rev() {
            self.push(StatementKind::StorageDead(local), span);
        }
    }

    fn drop_place(&mut self, place: Place, span: Span) {
        if self.mir.blocks[self.current.index()].terminator.is_some() {
            return;
        }
        let next = self.new_block();
        self.terminate(
            TerminatorKind::Drop {
                place,
                target: next,
            },
            span,
        );
        self.current = next;
    }

    /// Emits the drops of the scopes above `depth` without popping them (for `break`,
    /// `next` and `return`).
    fn exit_scopes_to(&mut self, depth: usize, temps_depth: usize, span: Span) {
        // The temporaries of the statements being left end first (spec §8.6 rule 1).
        let temps: Vec<Local> = self.stmt_temps[temps_depth.min(self.stmt_temps.len())..]
            .iter()
            .rev()
            .flat_map(|level| level.iter().rev().copied())
            .collect();
        for local in temps {
            self.drop_place(Place::local(local), span);
        }
        let mut drops = Vec::new();
        for scope in self.scopes[depth..].iter().rev() {
            drops.extend(scope.drops.iter().rev().copied());
        }
        for local in drops {
            self.drop_place(Place::local(local), span);
        }
    }

    fn exit_all_scopes(&mut self) {
        let span = self.mir.span;
        // Temporaries of an expression body (`def f = …`) end with it (spec §8.6 rule 1).
        while let Some(temps) = self.stmt_temps.pop() {
            for local in temps.into_iter().rev() {
                self.drop_place(Place::local(local), span);
            }
        }
        while self.scopes.len() > 1 {
            self.pop_scope(span);
        }
        if let Some(scope) = self.scopes.pop() {
            self.emit_scope_exit(&scope, span);
        }
        // Parameters are dropped when the function returns (spec §8.6 rule 1).
        let owned_args: Vec<Local> = (1..=self.mir.arg_count)
            .map(|i| Local(u32::try_from(i).unwrap_or(0)))
            .filter(|&l| {
                !matches!(
                    self.interner().kind(self.mir.local(l).ty),
                    TyKind::Ref { .. }
                ) && self.needs_drop(self.mir.local(l).ty)
            })
            .collect();
        for local in owned_args.into_iter().rev() {
            self.drop_place(Place::local(local), span);
        }
    }

    fn declare_binding(&mut self, binding: BindingId, ty: Ty, mutable: bool) -> Local {
        let binding = self.typed.canonical(binding);
        if let Some(place) = self.bindings.get(&binding)
            && place.proj.is_empty()
        {
            return place.local;
        }
        let span = Span::new(self.body.file, self.body.binding(binding).range);
        let local = self.new_local(ty, LocalKind::User(binding), mutable, span);
        self.push(StatementKind::StorageLive(local), span);
        if let Some(scope) = self.scopes.last_mut() {
            scope.locals.push(local);
        }
        if self.needs_drop(ty)
            && let Some(scope) = self.scopes.last_mut()
        {
            scope.drops.push(local);
        }
        self.bindings.insert(binding, Place::local(local));
        local
    }

    fn binding_place(&self, binding: BindingId) -> Place {
        let binding = self.typed.canonical(binding);
        self.bindings
            .get(&binding)
            .cloned()
            .unwrap_or_else(|| Place::local(Local(0)))
    }

    // ----- ownership pre-pass ---------------------------------------------------------------

    /// Records which bindings are used in owning positions and which closures escape
    /// (spec §8.1 rule 2, §8.4 rule 1).
    fn analyze_owning(&mut self) {
        let roots: Vec<ExprId> = self
            .body
            .root
            .into_iter()
            .chain(self.body.defaults.iter().flatten().copied())
            .collect();
        for root in roots {
            self.walk_owning(root, true);
        }
    }

    fn param_modes(&self, target: &CallTarget) -> (Option<Mode>, Vec<Mode>) {
        match target {
            CallTarget::Fn { def, .. } | CallTarget::Trait { method: def, .. } => {
                match self.items().fns.get(def) {
                    Some(sig) => (sig.receiver, sig.params.iter().map(|p| p.0).collect()),
                    None => (None, Vec::new()),
                }
            }
            CallTarget::Struct { .. } | CallTarget::Variant { .. } => (None, vec![Mode::Own; 64]),
            CallTarget::Value => (None, Vec::new()),
        }
    }

    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn walk_owning(&mut self, expr: ExprId, owning: bool) {
        let body = self.body;
        match body.expr(expr) {
            Expr::Name(_) => {
                if owning
                    && let Some(ExprInfo::Local(b)) = self.typed.exprs.get(&expr)
                    && !self.is_copy(self.coerced_ty(expr))
                {
                    self.owning_uses.insert(self.typed.canonical(*b));
                }
            }
            Expr::Closure {
                body: closure_body, ..
            } => {
                if owning {
                    self.escaping_closures.insert(expr);
                }
                self.walk_owning(*closure_body, true);
            }
            Expr::Call { callee, args }
            | Expr::MethodCall {
                receiver: callee,
                args,
                ..
            } => {
                let info = self.typed.exprs.get(&expr).cloned();
                let (receiver_mode, modes, sources) = match &info {
                    Some(ExprInfo::Call {
                        target,
                        args: sources,
                        ..
                    }) => {
                        let (r, m) = self.param_modes(target);
                        (r, m, sources.clone())
                    }
                    _ => (None, Vec::new(), Vec::new()),
                };
                let is_method = matches!(body.expr(expr), Expr::MethodCall { .. });
                self.walk_owning(*callee, is_method && receiver_mode == Some(Mode::Own));
                for (i, arg) in args.iter().enumerate() {
                    let param = sources.iter().position(|s| *s == ArgSource::Arg(i));
                    let own = param.is_some_and(|p| {
                        modes.get(p).copied().unwrap_or(Mode::Borrow) == Mode::Own
                    });
                    self.walk_owning(arg.value, own);
                }
            }
            Expr::Field { base, .. } => {
                let receiver_owned = match self.typed.exprs.get(&expr) {
                    Some(ExprInfo::Call { target, .. }) => {
                        self.param_modes(target).0 == Some(Mode::Own)
                    }
                    _ => false,
                };
                if matches!(self.typed.exprs.get(&expr), Some(ExprInfo::Field(_)))
                    && owning
                    && !self.is_copy(self.coerced_ty(expr))
                {
                    // Moving a field moves part of the base (spec §8.1 rule 3).
                    self.walk_owning(*base, true);
                } else {
                    self.walk_owning(*base, receiver_owned);
                }
            }
            Expr::TupleField { base, .. } | Expr::Try(base) => self.walk_owning(*base, owning),
            Expr::Index { base, indices, .. } => {
                self.walk_owning(*base, false);
                for &i in indices {
                    self.walk_owning(i, false);
                }
            }
            Expr::Unary { operand, .. } => self.walk_owning(*operand, false),
            Expr::Binary { lhs, rhs, .. } => {
                self.walk_owning(*lhs, false);
                self.walk_owning(*rhs, false);
            }
            Expr::Range { start, end, .. } => {
                self.walk_owning(*start, true);
                self.walk_owning(*end, true);
            }
            Expr::In { expr: e, .. } => self.walk_owning(*e, false),
            Expr::Interpolate(parts) => {
                for part in parts {
                    if let InterpolationPart::Expr(e) = part {
                        self.walk_owning(*e, false);
                    }
                }
            }
            Expr::Tuple(elements) | Expr::Array(elements) => {
                for &e in elements {
                    self.walk_owning(e, true);
                }
            }
            Expr::Map(entries) => {
                for &(k, v) in entries {
                    self.walk_owning(k, true);
                    self.walk_owning(v, true);
                }
            }
            Expr::Block { stmts } => {
                let count = stmts.len();
                for (i, stmt) in stmts.iter().enumerate() {
                    match stmt {
                        Stmt::Bind { value, .. } => self.walk_owning(*value, true),
                        Stmt::Assign {
                            place, value, op, ..
                        } => {
                            self.walk_owning(*place, false);
                            self.walk_owning(*value, op.is_none());
                        }
                        Stmt::Expr(e) => self.walk_owning(*e, owning && i + 1 == count),
                    }
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                self.walk_owning(*cond, false);
                self.walk_owning(*then_branch, owning);
                if let Some(e) = else_branch {
                    self.walk_owning(*e, owning);
                }
            }
            Expr::Case {
                scrutinee,
                arms,
                else_branch,
            } => {
                self.walk_owning(*scrutinee, false);
                for arm in arms {
                    if let Some(g) = arm.guard {
                        self.walk_owning(g, false);
                    }
                    self.walk_owning(arm.body, owning);
                }
                if let Some(e) = else_branch {
                    self.walk_owning(*e, owning);
                }
            }
            Expr::Loop(b) => self.walk_owning(*b, false),
            Expr::While { cond, body: b } => {
                self.walk_owning(*cond, false);
                self.walk_owning(*b, false);
            }
            Expr::For { iter, body: b, .. } => {
                let iterator = matches!(
                    self.typed.exprs.get(&expr),
                    Some(ExprInfo::For(ForKind::Iterator))
                );
                self.walk_owning(*iter, iterator);
                self.walk_owning(*b, false);
            }
            Expr::Return(v) | Expr::Break(v) => {
                if let Some(v) = v {
                    self.walk_owning(*v, true);
                }
            }
            Expr::Missing
            | Expr::Literal(_)
            | Expr::SelfValue
            | Expr::SelfType
            | Expr::Next
            | Expr::Unsupported(_) => {}
        }
    }

    // ----- places and operands -----------------------------------------------------------

    /// Adds a `Deref` for every `ref` layer of `ty`.
    fn deref_all(&self, mut place: Place, mut ty: Ty) -> Place {
        while let TyKind::Ref { inner, .. } = self.interner().kind(ty) {
            place = place.project(Proj::Deref);
            ty = inner;
        }
        place
    }

    /// The place `expr` denotes, if it is a place expression (before its coercion).
    fn place_of(&mut self, expr: ExprId) -> Option<Place> {
        match self.body.expr(expr) {
            Expr::Name(_) => match self.typed.exprs.get(&expr) {
                Some(ExprInfo::Local(b)) => Some(self.binding_place(*b)),
                _ => None,
            },
            Expr::SelfValue => self.self_place.clone(),
            &Expr::Field { base, .. } => match self.typed.exprs.get(&expr) {
                Some(&ExprInfo::Field(i)) => {
                    let base_place = self.as_place(base);
                    let base_place = self.deref_all(base_place, self.ty(base));
                    Some(base_place.project(Proj::Field(u32::try_from(i).unwrap_or(0))))
                }
                _ => None,
            },
            &Expr::TupleField { base, index } => {
                let base_place = self.as_place(base);
                let base_place = self.deref_all(base_place, self.ty(base));
                Some(base_place.project(Proj::Field(index)))
            }
            Expr::Index { base, indices, .. } => {
                let (base, index) = (*base, *indices.first()?);
                let base_ty = self.interner().strip_refs(self.ty(base));
                let is_array = matches!(self.interner().kind(base_ty), TyKind::Adt(d, _) if Some(d) == self.items().lang.get("Array"));
                if !is_array {
                    return None;
                }
                let base_place = self.as_place(base);
                let base_place = self.deref_all(base_place, self.ty(base));
                let index_op = self.as_operand(index);
                let span = self.span(index);
                let int = self.interner().int();
                let tmp = self.temp(int, span);
                self.assign(&Place::local(tmp), Rvalue::Use(index_op), span);
                Some(base_place.project(Proj::Index(tmp)))
            }
            _ => None,
        }
    }

    /// The place holding `expr`'s value after its coercion: the place itself for a place
    /// expression, or a temporary.
    fn as_place(&mut self, expr: ExprId) -> Place {
        if !matches!(
            self.typed.coercions.get(&expr),
            Some(Coercion::Autoref | Coercion::Unsize(_))
        ) && let Some(place) = self.place_of(expr)
        {
            return match self.typed.coercions.get(&expr) {
                Some(Coercion::Deref) => self.deref_all(place, self.ty(expr)),
                _ => place,
            };
        }
        let span = self.span(expr);
        if let Some(Coercion::Deref) = self.typed.coercions.get(&expr) {
            // A borrow produced by a call (`array_get(…)`): the place is behind it, not a
            // copy of its target.
            let ty = self.ty(expr);
            let tmp = self.temp(ty, span);
            let place = Place::local(tmp);
            self.expr_value_into(&place, expr);
            return self.deref_all(place, ty);
        }
        let ty = self.coerced_ty(expr);
        let tmp = self.temp(ty, span);
        let place = Place::local(tmp);
        self.expr_into_owning(&place, expr);
        place
    }

    /// `expr`'s value for an owning position: places are moved (or copied).
    fn as_operand(&mut self, expr: ExprId) -> Operand {
        let span = self.span(expr);
        match self.typed.coercions.get(&expr).copied() {
            Some(Coercion::Autoref) => {
                let place = if let Some(p) = self.place_of(expr) {
                    p
                } else {
                    let ty = self.ty(expr);
                    let tmp = self.temp(ty, span);
                    let p = Place::local(tmp);
                    self.expr_value_into(&p, expr);
                    p
                };
                let ty = self.coerced_ty(expr);
                let tmp = self.temp(ty, span);
                self.assign(
                    &Place::local(tmp),
                    Rvalue::Ref {
                        mutable: false,
                        place,
                    },
                    span,
                );
                Operand::Copy(Place::local(tmp))
            }
            Some(Coercion::Unsize(target)) => {
                let inner = self.raw_operand(expr);
                let tmp = self.temp(target, span);
                self.assign(&Place::local(tmp), Rvalue::Unsize(inner, target), span);
                Operand::Move(Place::local(tmp))
            }
            _ => {
                if let Some(place) = self.place_of(expr) {
                    let place = match self.typed.coercions.get(&expr) {
                        Some(Coercion::Deref) => self.deref_all(place, self.ty(expr)),
                        _ => place,
                    };
                    let ty = self.coerced_ty(expr);
                    if self.is_copy(ty) {
                        Operand::Copy(place)
                    } else {
                        Operand::Move(place)
                    }
                } else {
                    let ty = self.coerced_ty(expr);
                    if let Some(constant) = self.constant(expr) {
                        return Operand::Const(constant, ty);
                    }
                    let tmp = self.new_local(ty, LocalKind::Temp, true, span);
                    self.push(StatementKind::StorageLive(tmp), span);
                    let place = Place::local(tmp);
                    self.expr_value_into(&place, expr);
                    Operand::Move(place)
                }
            }
        }
    }

    /// `expr`'s value without its coercion.
    fn raw_operand(&mut self, expr: ExprId) -> Operand {
        let span = self.span(expr);
        if let Some(place) = self.place_of(expr) {
            let ty = self.ty(expr);
            return if self.is_copy(ty) {
                Operand::Copy(place)
            } else {
                Operand::Move(place)
            };
        }
        let ty = self.ty(expr);
        let tmp = self.new_local(ty, LocalKind::Temp, true, span);
        self.push(StatementKind::StorageLive(tmp), span);
        let place = Place::local(tmp);
        self.expr_value_into(&place, expr);
        Operand::Move(place)
    }

    /// A read of `expr` that does not move it (operands of operators).
    fn read_operand(&mut self, expr: ExprId) -> Operand {
        if let Some(place) = self.place_of(expr) {
            let place = self.deref_all(place, self.ty(expr));
            Operand::Copy(place)
        } else {
            let op = self.as_operand(expr);
            match op {
                Operand::Move(p) | Operand::Copy(p) => {
                    let ty = self.coerced_ty(expr);
                    Operand::Copy(self.deref_all(p, ty))
                }
                c @ Operand::Const(..) => c,
            }
        }
    }

    /// A borrow of `expr`'s place, for a borrow-mode or `mut` argument.
    fn borrow_operand(&mut self, expr: ExprId, mutable: bool) -> Operand {
        let span = self.span(expr);
        let ty = self.coerced_ty(expr);
        let place = self.as_place(expr);
        // An argument that is already a borrow of the parameter's type is passed on.
        let place = self.deref_all(place, ty);
        let inner = self.interner().strip_refs(ty);
        let ref_ty = self.interner().intern(TyKind::Ref { mutable, inner });
        let tmp = self.new_local(ref_ty, LocalKind::Temp, true, span);
        self.push(StatementKind::StorageLive(tmp), span);
        self.assign(&Place::local(tmp), Rvalue::Ref { mutable, place }, span);
        Operand::Copy(Place::local(tmp))
    }

    fn constant(&self, expr: ExprId) -> Option<Constant> {
        if self.typed.coercions.contains_key(&expr) {
            return None;
        }
        match self.body.expr(expr) {
            Expr::Literal(lit) => Some(self.literal(expr, lit, false)),
            Expr::Unary {
                op: UnaryOp::Neg,
                operand,
            } => match self.body.expr(*operand) {
                Expr::Literal(lit @ Literal::Int(_)) => Some(self.literal(*operand, lit, true)),
                _ => None,
            },
            _ => None,
        }
    }

    fn literal(&self, expr: ExprId, lit: &Literal, negative: bool) -> Constant {
        let ty = self.ty(expr);
        match lit {
            Literal::Int(v) => {
                let magnitude = i128::try_from(*v).unwrap_or_else(|_| v.cast_signed());
                let value = if negative {
                    magnitude.wrapping_neg()
                } else {
                    magnitude
                };
                match self.interner().kind(ty) {
                    TyKind::Int(t) => Constant::Int(value, t),
                    TyKind::Float(t) => {
                        #[allow(
                            clippy::cast_precision_loss,
                            reason = "integer literal used as a float"
                        )]
                        let f = value as f64;
                        Constant::Float(f, t)
                    }
                    _ => Constant::Int(value, loom_types::IntTy::I64),
                }
            }
            Literal::Float(v) => {
                let t = match self.interner().kind(ty) {
                    TyKind::Float(t) => t,
                    _ => loom_types::FloatTy::F64,
                };
                Constant::Float(if negative { -v } else { *v }, t)
            }
            Literal::Char(c) => Constant::Char(*c),
            Literal::String(s) => Constant::Str(Rc::clone(s)),
            Literal::Bytes(b) => Constant::Bytes(Rc::clone(b)),
            Literal::Bool(b) => Constant::Bool(*b),
        }
    }

    // ----- expressions -----------------------------------------------------------------------

    /// Evaluates `expr` (with its coercion) into `dest`, for an owning position.
    fn expr_into_owning(&mut self, dest: &Place, expr: ExprId) {
        let span = self.span(expr);
        if self.typed.coercions.contains_key(&expr) || self.place_of_is_simple(expr) {
            let op = self.as_operand(expr);
            self.assign(dest, Rvalue::Use(op), span);
        } else {
            self.expr_value_into(dest, expr);
        }
    }

    fn place_of_is_simple(&self, expr: ExprId) -> bool {
        match self.body.expr(expr) {
            Expr::Name(_) => matches!(self.typed.exprs.get(&expr), Some(ExprInfo::Local(_))),
            Expr::SelfValue | Expr::TupleField { .. } => true,
            Expr::Field { .. } => matches!(self.typed.exprs.get(&expr), Some(ExprInfo::Field(_))),
            Expr::Index { base, .. } => {
                let base_ty = self.interner().strip_refs(self.ty(*base));
                matches!(self.interner().kind(base_ty), TyKind::Adt(d, _) if Some(d) == self.items().lang.get("Array"))
            }
            _ => false,
        }
    }

    /// Evaluates the value of `expr` (without its coercion) into `dest`.
    #[allow(clippy::too_many_lines, reason = "one arm per expression kind")]
    fn expr_value_into(&mut self, dest: &Place, expr: ExprId) {
        let span = self.span(expr);
        let body = self.body;
        if self.place_of_is_simple(expr) {
            let op = self.raw_operand(expr);
            self.assign(dest, Rvalue::Use(op), span);
            return;
        }
        match body.expr(expr) {
            Expr::Missing | Expr::Unsupported(_) | Expr::SelfType => {
                self.terminate(
                    TerminatorKind::Panic(Rc::from("unsupported expression")),
                    span,
                );
                self.current = self.new_block();
            }
            Expr::Literal(lit) => {
                let c = self.literal(expr, lit, false);
                let ty = self.ty(expr);
                self.assign(dest, Rvalue::Use(Operand::Const(c, ty)), span);
            }
            Expr::Interpolate(parts) => self.interpolate(dest, parts, span),
            Expr::Name(_) | Expr::Field { .. } | Expr::SelfValue => self.path_value(dest, expr),
            Expr::TupleField { .. } => {}
            Expr::Call { .. } | Expr::MethodCall { .. } => {
                match self.typed.exprs.get(&expr).cloned() {
                    Some(ExprInfo::Call {
                        target,
                        receiver,
                        args,
                    }) => self.call(dest, expr, &target, receiver, &args),
                    _ => self.path_value(dest, expr),
                }
            }
            Expr::Index { base, indices, .. } => {
                // A map index: the value for the key, or a panic.
                let (base, key) = (*base, indices[0]);
                let map = self.borrow_operand(base, false);
                let key = self.borrow_operand(key, false);
                let value_ty = self.ty(expr);
                let ref_ty = self.interner().intern(TyKind::Ref {
                    mutable: false,
                    inner: value_ty,
                });
                let tmp = self.temp(ref_ty, span);
                let next = self.new_block();
                self.terminate(
                    TerminatorKind::Call {
                        callee: Callee::Builtin("map_index"),
                        args: vec![map, key],
                        dest: Place::local(tmp),
                        target: Some(next),
                    },
                    span,
                );
                self.current = next;
                let place = Place::local(tmp).project(Proj::Deref);
                let op = if self.is_copy(value_ty) {
                    Operand::Copy(place)
                } else {
                    Operand::Move(place)
                };
                self.assign(dest, Rvalue::Use(op), span);
            }
            &Expr::Unary { op, operand } => {
                if let Some(c) = self.constant(expr) {
                    let ty = self.ty(expr);
                    self.assign(dest, Rvalue::Use(Operand::Const(c, ty)), span);
                    return;
                }
                let v = self.read_operand(operand);
                self.assign(dest, Rvalue::Unary(op, v), span);
            }
            &Expr::Binary { op, lhs, rhs, .. } => self.binary(dest, expr, op, lhs, rhs),
            &Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let a = self.as_operand(start);
                let b = self.as_operand(end);
                let ty = self.ty(expr);
                let (def, subst) = match self.interner().kind(ty) {
                    TyKind::Adt(def, args) => (def, self.items().adt_subst(def, &args)),
                    _ => (DefId(0), Subst::new()),
                };
                let inclusive = Operand::Const(Constant::Bool(inclusive), self.interner().bool());
                self.assign(
                    dest,
                    Rvalue::Aggregate(AggregateKind::Struct(def, subst), vec![a, b, inclusive]),
                    span,
                );
            }
            &Expr::In { expr: tested, pat } => {
                let place = self.match_place(tested);
                let fail = self.new_block();
                let done = self.new_block();
                self.test_pattern(pat, &place.0, fail);
                self.bind_pattern(pat, &place.0, place.1);
                self.assign(
                    dest,
                    Rvalue::Use(Operand::Const(Constant::Bool(true), self.interner().bool())),
                    span,
                );
                self.goto(done, span);
                self.current = fail;
                self.assign(
                    dest,
                    Rvalue::Use(Operand::Const(
                        Constant::Bool(false),
                        self.interner().bool(),
                    )),
                    span,
                );
                self.goto(done, span);
                self.current = done;
            }
            &Expr::Try(inner) => self.try_expr(dest, expr, inner),
            Expr::Tuple(elements) => {
                if elements.is_empty() {
                    let unit = self.unit_operand();
                    self.assign(dest, Rvalue::Use(unit), span);
                    return;
                }
                let ops = elements.iter().map(|&e| self.as_operand(e)).collect();
                self.assign(dest, Rvalue::Aggregate(AggregateKind::Tuple, ops), span);
            }
            Expr::Array(elements) => {
                let ty = self.ty(expr);
                let elem = match self.interner().kind(ty) {
                    TyKind::Adt(_, args) => args
                        .first()
                        .copied()
                        .unwrap_or_else(|| self.interner().error()),
                    _ => self.interner().error(),
                };
                let ops = elements.iter().map(|&e| self.as_operand(e)).collect();
                self.assign(
                    dest,
                    Rvalue::Aggregate(AggregateKind::Array(elem), ops),
                    span,
                );
            }
            Expr::Map(entries) => {
                let ty = self.ty(expr);
                let (k, v) = match self.interner().kind(ty) {
                    TyKind::Adt(_, args) if args.len() == 2 => (args[0], args[1]),
                    _ => (self.interner().error(), self.interner().error()),
                };
                let mut ops = Vec::new();
                for &(key, value) in entries {
                    ops.push(self.as_operand(key));
                    ops.push(self.as_operand(value));
                }
                self.assign(dest, Rvalue::Aggregate(AggregateKind::Map(k, v), ops), span);
            }
            Expr::Closure {
                body: closure_body, ..
            } => self.closure(dest, expr, *closure_body),
            Expr::Block { stmts } => self.block(dest, stmts, span),
            &Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let then_block = self.new_block();
                let else_block = self.new_block();
                let join = self.new_block();
                self.cond_branch(cond, then_block, else_block);
                let has_value = else_branch.is_some();
                self.current = then_block;
                self.branch_into(dest, then_branch, has_value);
                self.goto(join, span);
                self.current = else_block;
                if let Some(e) = else_branch {
                    self.branch_into(dest, e, true);
                } else {
                    let unit = self.unit_operand();
                    self.assign(dest, Rvalue::Use(unit), span);
                }
                self.goto(join, span);
                self.current = join;
            }
            Expr::Case {
                scrutinee,
                arms,
                else_branch,
            } => {
                let (place, borrowed) = self.match_place(*scrutinee);
                let join = self.new_block();
                for arm in arms {
                    let next = self.new_block();
                    self.push_scope();
                    self.test_pattern(arm.pat, &place, next);
                    self.bind_pattern(arm.pat, &place, borrowed);
                    if let Some(guard) = arm.guard {
                        let g = self.read_operand(guard);
                        let pass = self.new_block();
                        self.terminate(
                            TerminatorKind::SwitchInt {
                                discr: g,
                                targets: vec![(0, next)],
                                otherwise: pass,
                            },
                            span,
                        );
                        self.current = pass;
                    }
                    self.branch_into(dest, arm.body, true);
                    self.pop_scope(span);
                    self.goto(join, span);
                    self.current = next;
                }
                match else_branch {
                    Some(e) => {
                        self.branch_into(dest, *e, true);
                        self.goto(join, span);
                    }
                    None => self.terminate(TerminatorKind::Unreachable, span),
                }
                self.current = join;
            }
            &Expr::Loop(loop_body) => {
                let head = self.new_block();
                let exit = self.new_block();
                self.goto(head, span);
                self.current = head;
                self.loops.push(LoopScope {
                    break_block: exit,
                    continue_block: head,
                    result: Some(dest.clone()),
                    scope_depth: self.scopes.len(),
                    temps_depth: self.stmt_temps.len(),
                });
                let tmp = self.unit_temp(span);
                self.expr_value_into(&tmp, loop_body);
                self.loops.pop();
                self.goto(head, span);
                self.current = exit;
            }
            &Expr::While {
                cond,
                body: loop_body,
            } => {
                let head = self.new_block();
                let body_block = self.new_block();
                let exit = self.new_block();
                self.goto(head, span);
                self.current = head;
                self.push_scope();
                self.cond_branch(cond, body_block, exit);
                self.current = body_block;
                self.loops.push(LoopScope {
                    break_block: exit,
                    continue_block: head,
                    result: None,
                    scope_depth: self.scopes.len() - 1,
                    temps_depth: self.stmt_temps.len(),
                });
                let tmp = self.unit_temp(span);
                self.expr_value_into(&tmp, loop_body);
                self.loops.pop();
                self.pop_scope(span);
                self.goto(head, span);
                self.current = exit;
                let unit = self.unit_operand();
                self.assign(dest, Rvalue::Use(unit), span);
            }
            &Expr::For {
                pat,
                iter,
                body: loop_body,
            } => self.for_loop(dest, expr, pat, iter, loop_body),
            &Expr::Return(value) => {
                let ret = Place::local(Local(0));
                if let Some(v) = value {
                    self.expr_into_owning(&ret, v);
                } else {
                    let unit = self.unit_operand();
                    self.assign(&ret, Rvalue::Use(unit), span);
                }
                self.exit_scopes_to(0, 0, span);
                self.terminate(TerminatorKind::Return, span);
                self.current = self.new_block();
            }
            &Expr::Break(value) => {
                let Some(target) = self.loops.last().map(|l| {
                    (
                        l.break_block,
                        l.result.clone(),
                        l.scope_depth,
                        l.temps_depth,
                    )
                }) else {
                    return;
                };
                if let (Some(v), Some(result)) = (value, &target.1) {
                    self.expr_into_owning(result, v);
                }
                self.exit_scopes_to(target.2, target.3, span);
                self.goto(target.0, span);
                self.current = self.new_block();
            }
            Expr::Next => {
                let Some((block, depth, temps_depth)) = self
                    .loops
                    .last()
                    .map(|l| (l.continue_block, l.scope_depth, l.temps_depth))
                else {
                    return;
                };
                self.exit_scopes_to(depth, temps_depth, span);
                self.goto(block, span);
                self.current = self.new_block();
            }
        }
    }

    /// Lowers a condition straight to control flow: pattern tests and `and` chains jump
    /// to `then_block` or `else_block` without building a `Bool`, so a binding is only
    /// considered initialized on the paths where it is (spec §9.3).
    fn cond_branch(&mut self, cond: ExprId, then_block: BlockId, else_block: BlockId) {
        let span = self.span(cond);
        match *self.body.expr(cond) {
            Expr::In { expr: tested, pat } if !self.typed.coercions.contains_key(&cond) => {
                let (place, borrowed) = self.match_place(tested);
                self.test_pattern(pat, &place, else_block);
                self.bind_pattern(pat, &place, borrowed);
                self.goto(then_block, span);
            }
            Expr::Binary {
                op: BinaryOp::And,
                lhs,
                rhs,
                ..
            } => {
                let middle = self.new_block();
                self.cond_branch(lhs, middle, else_block);
                self.current = middle;
                self.cond_branch(rhs, then_block, else_block);
            }
            _ => {
                let c = self.read_operand(cond);
                self.terminate(
                    TerminatorKind::SwitchInt {
                        discr: c,
                        targets: vec![(0, else_block)],
                        otherwise: then_block,
                    },
                    span,
                );
            }
        }
    }

    fn unit_temp(&mut self, span: Span) -> Place {
        let unit = self.interner().unit();
        let local = self.new_local(unit, LocalKind::Temp, true, span);
        Place::local(local)
    }

    /// Evaluates a branch of an `if` or `case`: its value if the construct has one.
    fn branch_into(&mut self, dest: &Place, branch: ExprId, has_value: bool) {
        if has_value {
            self.expr_into_owning(dest, branch);
        } else {
            let span = self.span(branch);
            let tmp = self.unit_temp(span);
            self.expr_value_into(&tmp, branch);
        }
    }

    fn block(&mut self, dest: &Place, stmts: &[Stmt], span: Span) {
        self.push_scope();
        let count = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            let last = i + 1 == count;
            self.stmt_temps.push(Vec::new());
            match *stmt {
                Stmt::Bind { binding, value, .. } => {
                    if let Some(&existing) = self.typed.assignments.get(&binding) {
                        let place = self.binding_place(existing);
                        let value_op = self.as_operand(value);
                        let value_span = self.span(value);
                        self.drop_place(place.clone(), value_span);
                        self.assign(&place, Rvalue::Use(value_op), value_span);
                    } else {
                        let ty = self.typed.binding_tys[self.typed.canonical(binding).index()];
                        let mutable = self.body.binding(binding).kind.is_mutable();
                        let value_op = self.as_operand(value);
                        let local = self.declare_binding(binding, ty, mutable);
                        let value_span = self.span(value);
                        self.assign(&Place::local(local), Rvalue::Use(value_op), value_span);
                    }
                    if last {
                        let unit = self.unit_operand();
                        self.assign(dest, Rvalue::Use(unit), span);
                    }
                }
                Stmt::Assign {
                    place, op, value, ..
                } => {
                    self.assignment(place, op, value);
                    if last {
                        let unit = self.unit_operand();
                        self.assign(dest, Rvalue::Use(unit), span);
                    }
                }
                Stmt::Expr(e) => {
                    if last {
                        self.expr_into_owning(dest, e);
                    } else {
                        let ty = self.ty(e);
                        let tmp = self.temp(ty, self.span(e));
                        self.expr_value_into(&Place::local(tmp), e);
                    }
                }
            }
            let temps = self.stmt_temps.pop().unwrap_or_default();
            for local in temps.into_iter().rev() {
                self.drop_place(Place::local(local), span);
            }
        }
        if count == 0 {
            let unit = self.unit_operand();
            self.assign(dest, Rvalue::Use(unit), span);
        }
        self.pop_scope(span);
    }

    fn assignment(&mut self, place_expr: ExprId, op: Option<BinaryOp>, value: ExprId) {
        let span = self.span(place_expr);
        // `map[key] = value` inserts (spec §7.4 rule 5).
        if let Expr::Index { base, indices, .. } = self.body.expr(place_expr)
            && op.is_none()
        {
            let base_ty = self.interner().strip_refs(self.ty(*base));
            if matches!(self.interner().kind(base_ty), TyKind::Adt(d, _) if Some(d) == self.items().lang.get("Map"))
            {
                let (base, key) = (*base, indices[0]);
                let map = self.borrow_operand(base, true);
                let key = self.as_operand(key);
                let value = self.as_operand(value);
                let unit = self.interner().unit();
                let tmp = self.unit_temp(span);
                let _ = unit;
                let next = self.new_block();
                self.terminate(
                    TerminatorKind::Call {
                        callee: Callee::Builtin("map_insert"),
                        args: vec![map, key, value],
                        dest: tmp,
                        target: Some(next),
                    },
                    span,
                );
                self.current = next;
                return;
            }
        }
        let place = match self.place_of(place_expr) {
            Some(p) => p,
            None => self.as_place(place_expr),
        };
        match op {
            None => {
                let v = self.as_operand(value);
                self.drop_place(place.clone(), span);
                self.assign(&place, Rvalue::Use(v), span);
            }
            Some(op) => {
                let v = self.read_operand(value);
                let ty = self.interner().strip_refs(self.ty(place_expr));
                let tmp = self.temp(ty, span);
                self.assign(
                    &Place::local(tmp),
                    Rvalue::Binary(op, Operand::Copy(place.clone()), v),
                    span,
                );
                self.assign(&place, Rvalue::Use(Operand::Copy(Place::local(tmp))), span);
            }
        }
    }

    fn path_value(&mut self, dest: &Place, expr: ExprId) {
        let span = self.span(expr);
        let ty = self.ty(expr);
        match self.typed.exprs.get(&expr).cloned() {
            Some(ExprInfo::FnValue { def, subst }) => {
                self.assign(
                    dest,
                    Rvalue::Use(Operand::Const(Constant::Fn { def, subst }, ty)),
                    span,
                );
            }
            Some(ExprInfo::Const { def }) => {
                self.assign(
                    dest,
                    Rvalue::Use(Operand::Const(Constant::Item(def), ty)),
                    span,
                );
            }
            Some(ExprInfo::UnitVariant { def }) => {
                let subst = match self.interner().kind(ty) {
                    TyKind::Adt(enum_def, args) => self.items().adt_subst(enum_def, &args),
                    _ => Subst::new(),
                };
                self.assign(
                    dest,
                    Rvalue::Aggregate(AggregateKind::Variant(def, subst), Vec::new()),
                    span,
                );
            }
            Some(ExprInfo::Call {
                target,
                receiver,
                args,
            }) => self.call(dest, expr, &target, receiver, &args),
            _ => {
                if let Some(place) = self.place_of(expr) {
                    let op = if self.is_copy(ty) {
                        Operand::Copy(place)
                    } else {
                        Operand::Move(place)
                    };
                    self.assign(dest, Rvalue::Use(op), span);
                }
            }
        }
    }

    fn interpolate(&mut self, dest: &Place, parts: &[InterpolationPart], span: Span) {
        let string = self.interner().string();
        let next = self.new_block();
        self.terminate(
            TerminatorKind::Call {
                callee: Callee::Builtin("string_new"),
                args: Vec::new(),
                dest: dest.clone(),
                target: Some(next),
            },
            span,
        );
        self.current = next;
        for part in parts {
            let text = match part {
                InterpolationPart::Text(t) => Operand::Const(Constant::Str(Rc::clone(t)), string),
                InterpolationPart::Expr(e) => {
                    let e = *e;
                    let ty = self.interner().strip_refs(self.ty(e));
                    let value = self.borrow_operand(e, false);
                    let tmp = self.temp(string, self.span(e));
                    let callee = self.display_callee(ty);
                    let next = self.new_block();
                    self.terminate(
                        TerminatorKind::Call {
                            callee,
                            args: vec![value],
                            dest: Place::local(tmp),
                            target: Some(next),
                        },
                        span,
                    );
                    self.current = next;
                    Operand::Move(Place::local(tmp))
                }
            };
            let target = Rvalue::Ref {
                mutable: true,
                place: dest.clone(),
            };
            let ref_ty = self.interner().intern(TyKind::Ref {
                mutable: true,
                inner: string,
            });
            let r = self.new_local(ref_ty, LocalKind::Temp, true, span);
            self.assign(&Place::local(r), target, span);
            let unit = self.unit_temp(span);
            let next = self.new_block();
            self.terminate(
                TerminatorKind::Call {
                    callee: Callee::Builtin("string_push"),
                    args: vec![Operand::Copy(Place::local(r)), text],
                    dest: unit,
                    target: Some(next),
                },
                span,
            );
            self.current = next;
        }
    }

    fn display_callee(&self, ty: Ty) -> Callee {
        let method = self
            .items()
            .lang
            .get("Display")
            .and_then(|t| self.items().traits.get(&t))
            .and_then(|t| t.methods.get(&Name::new("to_string")).copied())
            .unwrap_or(DefId(0));
        let mut subst = Subst::new();
        if let Some(display) = self.items().lang.get("Display") {
            subst.insert(Items::self_param(display), ty);
        }
        Callee::Trait {
            method,
            self_ty: ty,
            subst,
        }
    }

    fn lang_method(&self, trait_lang: &str, name: &str) -> Option<DefId> {
        let t = self.items().lang.get(trait_lang)?;
        self.items()
            .traits
            .get(&t)?
            .methods
            .get(&Name::new(name))
            .copied()
    }

    fn is_scalar(&self, ty: Ty) -> bool {
        matches!(
            self.interner().kind(self.interner().strip_refs(ty)),
            TyKind::Int(_)
                | TyKind::Float(_)
                | TyKind::Bool
                | TyKind::Char
                | TyKind::Unit
                | TyKind::Never
                | TyKind::Error
        )
    }

    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn binary(&mut self, dest: &Place, expr: ExprId, op: BinaryOp, lhs: ExprId, rhs: ExprId) {
        let span = self.span(expr);
        let bool_ty = self.interner().bool();
        match op {
            BinaryOp::And | BinaryOp::Or => {
                let a = self.read_operand(lhs);
                let rhs_block = self.new_block();
                let short = self.new_block();
                let join = self.new_block();
                let (zero_target, other) = if op == BinaryOp::And {
                    (short, rhs_block)
                } else {
                    (rhs_block, short)
                };
                self.terminate(
                    TerminatorKind::SwitchInt {
                        discr: a,
                        targets: vec![(0, zero_target)],
                        otherwise: other,
                    },
                    span,
                );
                self.current = short;
                self.assign(
                    dest,
                    Rvalue::Use(Operand::Const(Constant::Bool(op == BinaryOp::Or), bool_ty)),
                    span,
                );
                self.goto(join, span);
                self.current = rhs_block;
                let b = self.read_operand(rhs);
                self.assign(dest, Rvalue::Use(b), span);
                self.goto(join, span);
                self.current = join;
            }
            _ if (op.is_equality() || op.is_ordering()) && !self.is_scalar(self.ty(lhs)) => {
                let operand_ty = self.interner().strip_refs(self.ty(lhs));
                let a = self.borrow_operand(lhs, false);
                let b = self.borrow_operand(rhs, false);
                let (trait_lang, method) = if op.is_equality() {
                    ("Eq", "eq")
                } else {
                    ("Ord", "compare")
                };
                let method_def = self.lang_method(trait_lang, method).unwrap_or(DefId(0));
                let mut subst = Subst::new();
                if let Some(t) = self.items().lang.get(trait_lang) {
                    subst.insert(Items::self_param(t), operand_ty);
                }
                let result_ty = if op.is_equality() {
                    bool_ty
                } else {
                    self.items().lang.get("Ordering").map_or(bool_ty, |d| {
                        self.interner().intern(TyKind::Adt(d, Vec::new()))
                    })
                };
                let tmp = self.temp(result_ty, span);
                let next = self.new_block();
                self.terminate(
                    TerminatorKind::Call {
                        callee: Callee::Trait {
                            method: method_def,
                            self_ty: operand_ty,
                            subst,
                        },
                        args: vec![a, b],
                        dest: Place::local(tmp),
                        target: Some(next),
                    },
                    span,
                );
                self.current = next;
                match op {
                    BinaryOp::Eq => {
                        self.assign(dest, Rvalue::Use(Operand::Copy(Place::local(tmp))), span);
                    }
                    BinaryOp::Ne => self.assign(
                        dest,
                        Rvalue::Unary(UnaryOp::Not, Operand::Copy(Place::local(tmp))),
                        span,
                    ),
                    _ => {
                        // Ordering is Less = 0, Equal = 1, Greater = 2.
                        let int = self.interner().int();
                        let d = self.temp(int, span);
                        self.assign(
                            &Place::local(d),
                            Rvalue::Discriminant(Place::local(tmp)),
                            span,
                        );
                        let (cmp, value) = match op {
                            BinaryOp::Lt => (BinaryOp::Eq, 0),
                            BinaryOp::Le => (BinaryOp::Ne, 2),
                            BinaryOp::Gt => (BinaryOp::Eq, 2),
                            _ => (BinaryOp::Ne, 0),
                        };
                        let constant =
                            Operand::Const(Constant::Int(value, loom_types::IntTy::I64), int);
                        self.assign(
                            dest,
                            Rvalue::Binary(cmp, Operand::Copy(Place::local(d)), constant),
                            span,
                        );
                    }
                }
            }
            _ => {
                let a = self.read_operand(lhs);
                let b = self.read_operand(rhs);
                self.assign(dest, Rvalue::Binary(op, a, b), span);
            }
        }
    }

    // ----- calls ---------------------------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn call(
        &mut self,
        dest: &Place,
        expr: ExprId,
        target: &CallTarget,
        receiver: Option<ExprId>,
        sources: &[ArgSource],
    ) {
        let span = self.span(expr);
        let args: Vec<loom_hir::Arg> = match self.body.expr(expr) {
            Expr::Call { args, .. } | Expr::MethodCall { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let (receiver_mode, modes) = self.param_modes(target);
        let mut operands: Vec<Operand> = Vec::new();
        let callee_value = if let CallTarget::Value = target {
            let Expr::Call { callee, .. } = self.body.expr(expr) else {
                return;
            };
            let callee = *callee;
            Some(match self.place_of(callee) {
                Some(p) => Operand::Copy(p),
                None => self.as_operand(callee),
            })
        } else {
            None
        };
        // A `mut` receiver is borrowed after the arguments are evaluated, so
        // `list.push(list.length)` is accepted (Rust's two-phase borrows).
        let mut deferred_receiver: Option<(Place, Ty)> = None;
        if let Some(r) = receiver {
            match receiver_mode {
                Some(Mode::Own) => {
                    let op = self.as_operand(r);
                    operands.push(op);
                }
                Some(Mode::Mut) => {
                    let ty = self.coerced_ty(r);
                    let place = self.as_place(r);
                    let place = self.deref_all(place, ty);
                    deferred_receiver = Some((place, self.interner().strip_refs(ty)));
                    operands.push(self.unit_operand());
                }
                _ => {
                    let op = self.borrow_operand(r, false);
                    operands.push(op);
                }
            }
        }
        // Arguments in the order written (spec §7).
        let mut evaluated: Vec<Option<Operand>> = vec![None; args.len()];
        for (i, arg) in args.iter().enumerate() {
            let param = sources.iter().position(|s| *s == ArgSource::Arg(i));
            let mode = match target {
                CallTarget::Struct { .. } | CallTarget::Variant { .. } => Mode::Own,
                CallTarget::Value => Mode::Borrow,
                _ => param
                    .and_then(|p| modes.get(p).copied())
                    .unwrap_or(Mode::Borrow),
            };
            let op = match mode {
                Mode::Own => self.as_operand(arg.value),
                Mode::Mut => self.borrow_operand(arg.value, true),
                Mode::Borrow => self.borrow_operand(arg.value, false),
            };
            // Owned arguments are evaluated into temporaries so a default can borrow them.
            let op = match op {
                op @ Operand::Move(_) if sources.contains(&ArgSource::Default) => {
                    let ty = self.coerced_ty(arg.value);
                    let tmp = self.new_local(ty, LocalKind::Temp, true, span);
                    self.assign(&Place::local(tmp), Rvalue::Use(op), span);
                    Operand::Move(Place::local(tmp))
                }
                op => op,
            };
            evaluated[i] = Some(op);
        }
        if let Some((place, inner)) = deferred_receiver {
            let ref_ty = self.interner().intern(TyKind::Ref {
                mutable: true,
                inner,
            });
            let tmp = self.new_local(ref_ty, LocalKind::Temp, true, span);
            self.push(StatementKind::StorageLive(tmp), span);
            self.assign(
                &Place::local(tmp),
                Rvalue::Ref {
                    mutable: true,
                    place,
                },
                span,
            );
            operands[0] = Operand::Copy(Place::local(tmp));
        }
        match target {
            CallTarget::Struct { def, subst } => {
                let ops = sources
                    .iter()
                    .map(|s| match s {
                        ArgSource::Arg(i) => {
                            evaluated[*i].take().unwrap_or_else(|| self.unit_operand())
                        }
                        ArgSource::Default => self.unit_operand(),
                    })
                    .collect();
                self.assign(
                    dest,
                    Rvalue::Aggregate(AggregateKind::Struct(*def, subst.clone()), ops),
                    span,
                );
                return;
            }
            CallTarget::Variant { def, subst } => {
                let ops = sources
                    .iter()
                    .map(|s| match s {
                        ArgSource::Arg(i) => {
                            evaluated[*i].take().unwrap_or_else(|| self.unit_operand())
                        }
                        ArgSource::Default => self.unit_operand(),
                    })
                    .collect();
                self.assign(
                    dest,
                    Rvalue::Aggregate(AggregateKind::Variant(*def, subst.clone()), ops),
                    span,
                );
                return;
            }
            _ => {}
        }
        let (fn_def, fn_subst) = match target {
            CallTarget::Fn { def, subst }
            | CallTarget::Trait {
                method: def, subst, ..
            } => (*def, subst.clone()),
            _ => (DefId(0), Subst::new()),
        };
        for (p, source) in sources.iter().enumerate() {
            let op = match source {
                ArgSource::Arg(i) => evaluated[*i].take().unwrap_or_else(|| self.unit_operand()),
                ArgSource::Default => {
                    // The default sees the receiver and the arguments before it, borrowed.
                    let mut default_args: Vec<Operand> = Vec::new();
                    for prior in &operands {
                        default_args.push(self.reborrow(prior, span));
                    }
                    let ty = self
                        .items()
                        .fns
                        .get(&fn_def)
                        .map_or_else(|| self.interner().error(), |s| s.params[p].2);
                    // A temporary of the call's statement: dropped after it (spec §8.6).
                    let tmp = self.temp(ty, span);
                    let next = self.new_block();
                    self.terminate(
                        TerminatorKind::Call {
                            callee: Callee::Default {
                                def: fn_def,
                                index: u32::try_from(p).unwrap_or(0),
                                subst: fn_subst.clone(),
                            },
                            args: default_args,
                            dest: Place::local(tmp),
                            target: Some(next),
                        },
                        span,
                    );
                    self.current = next;
                    let mode = modes.get(p).copied().unwrap_or(Mode::Borrow);
                    if mode == Mode::Own {
                        Operand::Move(Place::local(tmp))
                    } else {
                        let mutable = mode == Mode::Mut;
                        let ref_ty = self.interner().intern(TyKind::Ref { mutable, inner: ty });
                        let r = self.new_local(ref_ty, LocalKind::Temp, true, span);
                        self.assign(
                            &Place::local(r),
                            Rvalue::Ref {
                                mutable,
                                place: Place::local(tmp),
                            },
                            span,
                        );
                        Operand::Copy(Place::local(r))
                    }
                }
            };
            operands.push(op);
        }
        let callee = match target {
            CallTarget::Fn { def, subst } => Callee::Fn {
                def: *def,
                subst: subst.clone(),
            },
            CallTarget::Trait {
                method,
                self_ty,
                subst,
            } => Callee::Trait {
                method: *method,
                self_ty: *self_ty,
                subst: subst.clone(),
            },
            _ => Callee::Value(callee_value.unwrap_or_else(|| self.unit_operand())),
        };
        let returns = !matches!(self.interner().kind(self.ty(expr)), TyKind::Never);
        let next = self.new_block();
        self.terminate(
            TerminatorKind::Call {
                callee,
                args: operands,
                dest: dest.clone(),
                target: returns.then_some(next),
            },
            span,
        );
        self.current = next;
    }

    /// A shared borrow of what `op` refers to or holds, for a default value's arguments.
    fn reborrow(&mut self, op: &Operand, span: Span) -> Operand {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                let ty = self.place_ty(p);
                if let TyKind::Ref { inner, .. } = self.interner().kind(ty) {
                    let ref_ty = self.interner().intern(TyKind::Ref {
                        mutable: false,
                        inner,
                    });
                    let tmp = self.new_local(ref_ty, LocalKind::Temp, true, span);
                    self.assign(
                        &Place::local(tmp),
                        Rvalue::Ref {
                            mutable: false,
                            place: p.project(Proj::Deref),
                        },
                        span,
                    );
                    Operand::Copy(Place::local(tmp))
                } else {
                    let ref_ty = self.interner().intern(TyKind::Ref {
                        mutable: false,
                        inner: ty,
                    });
                    let tmp = self.new_local(ref_ty, LocalKind::Temp, true, span);
                    self.assign(
                        &Place::local(tmp),
                        Rvalue::Ref {
                            mutable: false,
                            place: p.clone(),
                        },
                        span,
                    );
                    Operand::Copy(Place::local(tmp))
                }
            }
            c @ Operand::Const(..) => c.clone(),
        }
    }

    /// The type of a place (projections applied to its local's type).
    fn place_ty(&self, place: &Place) -> Ty {
        crate::place_ty(self.cx.hir, self.items(), self.interner(), &self.mir, place)
    }

    // ----- `?`, `for`, closures --------------------------------------------------------------

    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn try_expr(&mut self, dest: &Place, expr: ExprId, inner: ExprId) {
        let span = self.span(expr);
        let operand = self.as_operand(inner);
        let inner_ty = self.interner().strip_refs(self.ty(inner));
        let tmp = self.new_local(inner_ty, LocalKind::Temp, true, span);
        self.assign(&Place::local(tmp), Rvalue::Use(operand), span);
        let TyKind::Adt(enum_def, args) = self.interner().kind(inner_ty) else {
            return;
        };
        let DefKind::Enum(data) = &self.cx.hir.def(enum_def).kind else {
            return;
        };
        let (good, bad) = (data.variants[0], data.variants[1]);
        let int = self.interner().int();
        let d = self.temp(int, span);
        self.assign(
            &Place::local(d),
            Rvalue::Discriminant(Place::local(tmp)),
            span,
        );
        let good_block = self.new_block();
        let bad_block = self.new_block();
        self.terminate(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::local(d)),
                targets: vec![(0, good_block)],
                otherwise: bad_block,
            },
            span,
        );
        let _ = args;
        // Failure: return `Err(From.from(e))` or `None`.
        self.current = bad_block;
        let ret_ty = self.mir.local(Local(0)).ty;
        let ret_subst = match self.interner().kind(ret_ty) {
            TyKind::Adt(d, a) => self.items().adt_subst(d, &a),
            _ => Subst::new(),
        };
        let ret_variant = match self.interner().kind(ret_ty) {
            TyKind::Adt(d, _) => match &self.cx.hir.def(d).kind {
                DefKind::Enum(e) => e.variants[1],
                _ => bad,
            },
            _ => bad,
        };
        let ret = Place::local(Local(0));
        match self.typed.exprs.get(&expr).cloned() {
            Some(ExprInfo::Try {
                result: true,
                convert,
            }) => {
                let error = Operand::Move(
                    Place::local(tmp)
                        .project(Proj::Downcast(bad))
                        .project(Proj::Field(0)),
                );
                let error = match convert {
                    Some(CallTarget::Trait {
                        method,
                        self_ty,
                        subst,
                    }) => {
                        let converted = self.new_local(self_ty, LocalKind::Temp, true, span);
                        let next = self.new_block();
                        self.terminate(
                            TerminatorKind::Call {
                                callee: Callee::Trait {
                                    method,
                                    self_ty,
                                    subst,
                                },
                                args: vec![error],
                                dest: Place::local(converted),
                                target: Some(next),
                            },
                            span,
                        );
                        self.current = next;
                        Operand::Move(Place::local(converted))
                    }
                    _ => error,
                };
                self.assign(
                    &ret,
                    Rvalue::Aggregate(AggregateKind::Variant(ret_variant, ret_subst), vec![error]),
                    span,
                );
            }
            _ => {
                self.assign(
                    &ret,
                    Rvalue::Aggregate(AggregateKind::Variant(ret_variant, ret_subst), Vec::new()),
                    span,
                );
            }
        }
        self.exit_scopes_to(0, 0, span);
        self.terminate(TerminatorKind::Return, span);
        self.current = good_block;
        let value = Place::local(tmp)
            .project(Proj::Downcast(good))
            .project(Proj::Field(0));
        let ty = self.ty(expr);
        let op = if self.is_copy(ty) {
            Operand::Copy(value)
        } else {
            Operand::Move(value)
        };
        self.assign(dest, Rvalue::Use(op), span);
    }

    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn for_loop(
        &mut self,
        dest: &Place,
        expr: ExprId,
        pat: PatId,
        iter: ExprId,
        loop_body: ExprId,
    ) {
        let span = self.span(expr);
        let iterable_ty = self.interner().strip_refs(self.ty(iter));
        let kind = self.typed.exprs.get(&expr).cloned();
        let iterator = self
            .lang_method("Iterator", "next_item")
            .unwrap_or(DefId(0));
        let (it_local, it_ty) = if matches!(kind, Some(ExprInfo::For(ForKind::Iterable))) {
            let method = self.lang_method("Iterable", "iter").unwrap_or(DefId(0));
            let it_ty = self.projection("Iterable", "Iter", iterable_ty);
            let local = self.new_local(it_ty, LocalKind::Temp, true, span);
            let arg = self.borrow_operand(iter, false);
            let mut subst = Subst::new();
            if let Some(t) = self.items().lang.get("Iterable") {
                subst.insert(Items::self_param(t), iterable_ty);
            }
            let next = self.new_block();
            self.terminate(
                TerminatorKind::Call {
                    callee: Callee::Trait {
                        method,
                        self_ty: iterable_ty,
                        subst,
                    },
                    args: vec![arg],
                    dest: Place::local(local),
                    target: Some(next),
                },
                span,
            );
            self.current = next;
            (local, it_ty)
        } else {
            let op = self.as_operand(iter);
            let local = self.new_local(iterable_ty, LocalKind::Temp, true, span);
            self.assign(&Place::local(local), Rvalue::Use(op), span);
            (local, iterable_ty)
        };
        let owned_iterator = !matches!(self.interner().kind(it_ty), TyKind::Ref { .. });
        if owned_iterator && let Some(scope) = self.scopes.last_mut() {
            scope.drops.push(it_local);
        }
        let item_ty = self.projection("Iterator", "Item", it_ty);
        let option_ty = self.items().lang.get("Option").map_or(item_ty, |d| {
            self.interner().intern(TyKind::Adt(d, vec![item_ty]))
        });
        let head = self.new_block();
        let body_block = self.new_block();
        let exit = self.new_block();
        self.goto(head, span);
        self.current = head;
        let next_local = self.new_local(option_ty, LocalKind::Temp, true, span);
        let it_ref = {
            let ref_ty = self.interner().intern(TyKind::Ref {
                mutable: true,
                inner: it_ty,
            });
            let r = self.new_local(ref_ty, LocalKind::Temp, true, span);
            self.assign(
                &Place::local(r),
                Rvalue::Ref {
                    mutable: true,
                    place: Place::local(it_local),
                },
                span,
            );
            r
        };
        let mut subst = Subst::new();
        if let Some(t) = self.items().lang.get("Iterator") {
            subst.insert(Items::self_param(t), it_ty);
        }
        let after_call = self.new_block();
        self.terminate(
            TerminatorKind::Call {
                callee: Callee::Trait {
                    method: iterator,
                    self_ty: it_ty,
                    subst,
                },
                args: vec![Operand::Copy(Place::local(it_ref))],
                dest: Place::local(next_local),
                target: Some(after_call),
            },
            span,
        );
        self.current = after_call;
        let int = self.interner().int();
        let d = self.new_local(int, LocalKind::Temp, true, span);
        self.assign(
            &Place::local(d),
            Rvalue::Discriminant(Place::local(next_local)),
            span,
        );
        self.terminate(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::local(d)),
                targets: vec![(0, body_block)],
                otherwise: exit,
            },
            span,
        );
        self.current = body_block;
        self.push_scope();
        let some = self
            .items()
            .lang
            .get("Option")
            .and_then(|d| match &self.cx.hir.def(d).kind {
                DefKind::Enum(e) => e.variants.first().copied(),
                _ => None,
            })
            .unwrap_or(DefId(0));
        let item_place = Place::local(next_local)
            .project(Proj::Downcast(some))
            .project(Proj::Field(0));
        let fail = self.new_block();
        self.test_pattern(pat, &item_place, fail);
        self.bind_pattern(pat, &item_place, false);
        self.loops.push(LoopScope {
            break_block: exit,
            continue_block: head,
            result: None,
            scope_depth: self.scopes.len() - 1,
            temps_depth: self.stmt_temps.len(),
        });
        let tmp = self.unit_temp(span);
        self.expr_value_into(&tmp, loop_body);
        self.loops.pop();
        self.pop_scope(span);
        self.goto(head, span);
        self.current = fail;
        self.terminate(TerminatorKind::Unreachable, span);
        self.current = exit;
        let unit = self.unit_operand();
        self.assign(dest, Rvalue::Use(unit), span);
    }

    /// The normalized associated type `assoc` of `trait_lang` for `ty`.
    fn projection(&self, trait_lang: &str, assoc: &str, ty: Ty) -> Ty {
        let interner = self.interner();
        let Some(trait_) = self.items().lang.get(trait_lang) else {
            return interner.error();
        };
        let Some(&assoc_def) = self
            .items()
            .traits
            .get(&trait_)
            .and_then(|t| t.assoc_types.get(&Name::new(assoc)))
        else {
            return interner.error();
        };
        let projection = interner.intern(TyKind::Projection {
            trait_,
            args: vec![ty],
            assoc: assoc_def,
        });
        let mut table = InferTable::new();
        Solver {
            hir: self.cx.hir,
            items: self.items(),
            interner,
            table: &mut table,
        }
        .normalize(projection, &self.env)
    }

    fn closure(&mut self, dest: &Place, expr: ExprId, closure_body: ExprId) {
        let span = self.span(expr);
        let escaping = self.escaping_closures.contains(&expr);
        // Bindings declared inside the closure, and those it uses or mutates.
        let mut declared = HashSet::new();
        let Expr::Closure { params, .. } = self.body.expr(expr) else {
            return;
        };
        for param in params.iter().flatten() {
            collect_pattern_bindings(self.body, param.pat, &mut declared);
        }
        collect_declared(self.body, closure_body, &mut declared);
        let mut used = Vec::new();
        let mut mutated = HashSet::new();
        collect_uses(
            self.body,
            self.typed,
            closure_body,
            &mut used,
            &mut mutated,
            &self.items().fns,
        );
        let mut captures = Vec::new();
        let mut seen = HashSet::new();
        for b in used {
            let b = self.typed.canonical(b);
            if declared.contains(&b) || !seen.insert(b) || !self.bindings.contains_key(&b) {
                continue;
            }
            let ty = self.typed.binding_tys[b.index()];
            let by_ref = if escaping {
                None
            } else {
                Some(mutated.contains(&b))
            };
            captures.push(Capture {
                binding: b,
                by_ref,
                ty,
            });
        }
        let ops: Vec<Operand> = captures
            .iter()
            .map(|c| {
                let place = self.binding_place(c.binding);
                match c.by_ref {
                    None => {
                        let ty = c.ty;
                        if self.is_copy(ty) {
                            Operand::Copy(place)
                        } else {
                            Operand::Move(place)
                        }
                    }
                    Some(mutable) => {
                        let ref_ty = self.interner().intern(TyKind::Ref {
                            mutable,
                            inner: c.ty,
                        });
                        let tmp = self.new_local(ref_ty, LocalKind::Temp, true, span);
                        self.assign(&Place::local(tmp), Rvalue::Ref { mutable, place }, span);
                        Operand::Copy(Place::local(tmp))
                    }
                }
            })
            .collect();
        let source = MirSource::Closure(self.body_id, expr);
        self.assign(
            dest,
            Rvalue::Aggregate(AggregateKind::Closure(source), ops),
            span,
        );
        self.closures.push(Job::Closure {
            body: self.body_id,
            expr,
            captures,
            escaping,
            outer_by_ref: self.by_ref.clone(),
        });
    }

    // ----- patterns --------------------------------------------------------------------------

    /// The place a `case` or pattern test matches, and whether it is borrowed (a place
    /// expression) rather than a temporary.
    fn match_place(&mut self, scrutinee: ExprId) -> (Place, bool) {
        if let Some(place) = self.place_of(scrutinee) {
            let place = self.deref_all(place, self.ty(scrutinee));
            return (place, true);
        }
        let place = self.as_place(scrutinee);
        let place = self.deref_all(place, self.coerced_ty(scrutinee));
        (place, false)
    }

    /// Emits tests that jump to `fail` unless `place` matches `pat`.
    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn test_pattern(&mut self, pat: PatId, place: &Place, fail: BlockId) {
        let span = self.body.pat_span(pat);
        let pat_ty = self.typed.pat_tys[pat.index()];
        let place = self.deref_all(place.clone(), self.place_ty(place));
        let bool_ty = self.interner().bool();
        match self.body.pat(pat).clone() {
            Pat::Missing | Pat::Wild | Pat::Bind(_) | Pat::Rest => {}
            Pat::Literal { lit, negative } => {
                let ty = self.interner().strip_refs(pat_ty);
                let c = match lit {
                    Literal::Int(v) => {
                        let magnitude = i128::try_from(v).unwrap_or(i128::MAX);
                        let value = if negative { -magnitude } else { magnitude };
                        match self.interner().kind(ty) {
                            TyKind::Int(t) => Constant::Int(value, t),
                            _ => Constant::Int(value, loom_types::IntTy::I64),
                        }
                    }
                    Literal::Float(f) => {
                        Constant::Float(if negative { -f } else { f }, loom_types::FloatTy::F64)
                    }
                    Literal::Char(c) => Constant::Char(c),
                    Literal::String(s) => Constant::Str(s),
                    Literal::Bytes(b) => Constant::Bytes(b),
                    Literal::Bool(b) => Constant::Bool(b),
                };
                let tmp = self.temp(bool_ty, span);
                self.assign(
                    &Place::local(tmp),
                    Rvalue::Binary(BinaryOp::Eq, Operand::Copy(place), Operand::Const(c, ty)),
                    span,
                );
                self.branch_unless(Place::local(tmp), fail, span);
            }
            Pat::Range { lo, hi, inclusive } => {
                let ty = self.interner().strip_refs(pat_ty);
                let bound = |b: &(Literal, bool)| match &b.0 {
                    Literal::Int(v) => {
                        let m = i128::try_from(*v).unwrap_or(i128::MAX);
                        let t = match self.interner().kind(ty) {
                            TyKind::Int(t) => t,
                            _ => loom_types::IntTy::I64,
                        };
                        Constant::Int(if b.1 { -m } else { m }, t)
                    }
                    Literal::Char(c) => Constant::Char(*c),
                    _ => Constant::Unit,
                };
                let (lo, hi) = (bound(&lo), bound(&hi));
                let a = self.temp(bool_ty, span);
                self.assign(
                    &Place::local(a),
                    Rvalue::Binary(
                        BinaryOp::Ge,
                        Operand::Copy(place.clone()),
                        Operand::Const(lo, ty),
                    ),
                    span,
                );
                self.branch_unless(Place::local(a), fail, span);
                let b = self.temp(bool_ty, span);
                let op = if inclusive {
                    BinaryOp::Le
                } else {
                    BinaryOp::Lt
                };
                self.assign(
                    &Place::local(b),
                    Rvalue::Binary(op, Operand::Copy(place), Operand::Const(hi, ty)),
                    span,
                );
                self.branch_unless(Place::local(b), fail, span);
            }
            Pat::Tuple(elements) => {
                let rest = elements
                    .iter()
                    .position(|&e| matches!(self.body.pat(e), Pat::Rest));
                let arity = match self.interner().kind(self.interner().strip_refs(pat_ty)) {
                    TyKind::Tuple(ts) => ts.len(),
                    _ => elements.len(),
                };
                for (i, &e) in elements.iter().enumerate() {
                    if Some(i) == rest {
                        continue;
                    }
                    let index = match rest {
                        Some(r) if i > r => arity - (elements.len() - i),
                        _ => i,
                    };
                    let sub = place.project(Proj::Field(u32::try_from(index).unwrap_or(0)));
                    self.test_pattern(e, &sub, fail);
                }
            }
            Pat::Path(_) | Pat::TupleStruct { .. } | Pat::Struct { .. } => {
                self.test_path_pattern(pat, &place, fail, span);
            }
            Pat::Or(alternatives) => {
                let success = self.new_block();
                for &alternative in &alternatives {
                    let next = self.new_block();
                    self.test_pattern(alternative, &place, next);
                    self.goto(success, span);
                    self.current = next;
                }
                self.goto(fail, span);
                self.current = success;
            }
        }
    }

    fn branch_unless(&mut self, condition: Place, fail: BlockId, span: Span) {
        let next = self.new_block();
        self.terminate(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(condition),
                targets: vec![(0, fail)],
                otherwise: next,
            },
            span,
        );
        self.current = next;
    }

    fn test_path_pattern(&mut self, pat: PatId, place: &Place, fail: BlockId, span: Span) {
        match self.typed.pats.get(&pat).cloned() {
            Some(PatInfo::Variant(variant)) => {
                let index = match &self.cx.hir.def(variant).kind {
                    DefKind::Variant(v) => v.index,
                    _ => 0,
                };
                let int = self.interner().int();
                let d = self.temp(int, span);
                self.assign(&Place::local(d), Rvalue::Discriminant(place.clone()), span);
                let next = self.new_block();
                self.terminate(
                    TerminatorKind::SwitchInt {
                        discr: Operand::Copy(Place::local(d)),
                        targets: vec![(u128::from(index), next)],
                        otherwise: fail,
                    },
                    span,
                );
                self.current = next;
                let inner = place.project(Proj::Downcast(variant));
                self.test_fields(pat, &inner, fail, Some(variant));
            }
            Some(PatInfo::Struct(def)) => {
                let _ = def;
                self.test_fields(pat, place, fail, None);
            }
            Some(PatInfo::Const(def)) => {
                let ty = self.interner().strip_refs(self.typed.pat_tys[pat.index()]);
                let bool_ty = self.interner().bool();
                let tmp = self.temp(bool_ty, span);
                self.assign(
                    &Place::local(tmp),
                    Rvalue::Binary(
                        BinaryOp::Eq,
                        Operand::Copy(place.clone()),
                        Operand::Const(Constant::Item(def), ty),
                    ),
                    span,
                );
                self.branch_unless(Place::local(tmp), fail, span);
            }
            None => {}
        }
    }

    fn field_names(&self, pat: PatId, variant: Option<DefId>) -> Vec<Name> {
        match variant {
            Some(v) => match self.items().variants.get(&v) {
                Some(loom_check::VariantTy::Named(fields)) => {
                    fields.iter().map(|f| f.name.clone()).collect()
                }
                _ => Vec::new(),
            },
            None => match self.typed.pats.get(&pat) {
                Some(PatInfo::Struct(def)) => self
                    .items()
                    .fields
                    .get(def)
                    .map(|fs| fs.iter().map(|f| f.name.clone()).collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
        }
    }

    fn test_fields(&mut self, pat: PatId, place: &Place, fail: BlockId, variant: Option<DefId>) {
        match self.body.pat(pat).clone() {
            Pat::TupleStruct { pats, .. } => {
                for (i, &p) in pats.iter().enumerate() {
                    let sub = place.project(Proj::Field(u32::try_from(i).unwrap_or(0)));
                    self.test_pattern(p, &sub, fail);
                }
            }
            Pat::Struct { fields, .. } => {
                let names = self.field_names(pat, variant);
                for (name, _, p) in fields {
                    if let Some(i) = names.iter().position(|n| *n == name) {
                        let sub = place.project(Proj::Field(u32::try_from(i).unwrap_or(0)));
                        self.test_pattern(p, &sub, fail);
                    }
                }
            }
            _ => {}
        }
    }

    /// Binds the names of `pat` to parts of `place`. Bindings borrow when the matched
    /// value is borrowed or `place` is a borrowed place, unless they are moved in their
    /// arm (spec §8.3 rule 6).
    #[allow(clippy::too_many_lines, reason = "one case per MIR construct")]
    fn bind_pattern(&mut self, pat: PatId, place: &Place, borrowed_place: bool) {
        let span = self.body.pat_span(pat);
        let pat_ty = self.typed.pat_tys[pat.index()];
        let place_ty = self.place_ty(place);
        let derefed = self.deref_all(place.clone(), place_ty);
        match self.body.pat(pat).clone() {
            Pat::Bind(binding) => {
                let binding = self.typed.canonical(binding);
                let ty = self.typed.binding_tys[binding.index()];
                let through_ref = matches!(self.interner().kind(ty), TyKind::Ref { .. })
                    && !matches!(self.interner().kind(place_ty), TyKind::Ref { .. });
                let _ = pat_ty;
                let mutable = self.body.binding(binding).kind.is_mutable();
                if through_ref {
                    // Matched through a borrow: the binding is a borrow of the part.
                    let local = self.declare_binding(binding, ty, false);
                    let m = matches!(self.interner().kind(ty), TyKind::Ref { mutable: true, .. });
                    self.assign(
                        &Place::local(local),
                        Rvalue::Ref {
                            mutable: m,
                            place: derefed.clone(),
                        },
                        span,
                    );
                } else if borrowed_place
                    && !self.is_copy(ty)
                    && !self.owning_uses.contains(&binding)
                {
                    // An owned place matched without moving: borrow the part.
                    let ref_ty = self.interner().intern(TyKind::Ref {
                        mutable: false,
                        inner: ty,
                    });
                    let local = self.new_local(ref_ty, LocalKind::User(binding), false, span);
                    self.push(StatementKind::StorageLive(local), span);
                    self.assign(
                        &Place::local(local),
                        Rvalue::Ref {
                            mutable: false,
                            place: place.clone(),
                        },
                        span,
                    );
                    self.bindings
                        .insert(binding, Place::local(local).project(Proj::Deref));
                    self.by_ref.insert(binding);
                } else {
                    let local = self.declare_binding(binding, ty, mutable);
                    let op = if self.is_copy(ty) {
                        Operand::Copy(place.clone())
                    } else {
                        Operand::Move(place.clone())
                    };
                    self.assign(&Place::local(local), Rvalue::Use(op), span);
                }
            }
            Pat::Tuple(elements) => {
                let rest = elements
                    .iter()
                    .position(|&e| matches!(self.body.pat(e), Pat::Rest));
                let arity = match self.interner().kind(self.interner().strip_refs(pat_ty)) {
                    TyKind::Tuple(ts) => ts.len(),
                    _ => elements.len(),
                };
                for (i, &e) in elements.iter().enumerate() {
                    if Some(i) == rest {
                        continue;
                    }
                    let index = match rest {
                        Some(r) if i > r => arity - (elements.len() - i),
                        _ => i,
                    };
                    let sub = derefed.project(Proj::Field(u32::try_from(index).unwrap_or(0)));
                    self.bind_pattern(e, &sub, borrowed_place);
                }
            }
            Pat::TupleStruct { pats, .. } => {
                let inner = self.variant_place(pat, &derefed);
                for (i, &p) in pats.iter().enumerate() {
                    let sub = inner.project(Proj::Field(u32::try_from(i).unwrap_or(0)));
                    self.bind_pattern(p, &sub, borrowed_place);
                }
            }
            Pat::Struct { fields, .. } => {
                let variant = match self.typed.pats.get(&pat) {
                    Some(PatInfo::Variant(v)) => Some(*v),
                    _ => None,
                };
                let names = self.field_names(pat, variant);
                let inner = self.variant_place(pat, &derefed);
                for (name, _, p) in fields {
                    if let Some(i) = names.iter().position(|n| *n == name) {
                        let sub = inner.project(Proj::Field(u32::try_from(i).unwrap_or(0)));
                        self.bind_pattern(p, &sub, borrowed_place);
                    }
                }
            }
            Pat::Or(alternatives) => {
                // Every alternative binds the same names; the first defines the locals.
                if let Some(&first) = alternatives.first() {
                    self.bind_pattern(first, place, borrowed_place);
                }
            }
            Pat::Missing
            | Pat::Wild
            | Pat::Rest
            | Pat::Literal { .. }
            | Pat::Range { .. }
            | Pat::Path(_) => {}
        }
    }

    fn variant_place(&self, pat: PatId, place: &Place) -> Place {
        match self.typed.pats.get(&pat) {
            Some(PatInfo::Variant(v)) => place.project(Proj::Downcast(*v)),
            _ => place.clone(),
        }
    }
}

fn collect_pattern_bindings(body: &Body, pat: PatId, out: &mut HashSet<BindingId>) {
    match body.pat(pat) {
        Pat::Bind(b) => {
            out.insert(*b);
        }
        Pat::Tuple(ps) | Pat::Or(ps) | Pat::TupleStruct { pats: ps, .. } => {
            for &p in ps {
                collect_pattern_bindings(body, p, out);
            }
        }
        Pat::Struct { fields, .. } => {
            for &(_, _, p) in fields {
                collect_pattern_bindings(body, p, out);
            }
        }
        _ => {}
    }
}

/// Bindings declared anywhere inside `expr`.
fn collect_declared(body: &Body, expr: ExprId, out: &mut HashSet<BindingId>) {
    walk_exprs(body, expr, &mut |e| match body.expr(e) {
        Expr::Block { stmts } => {
            for stmt in stmts {
                if let Stmt::Bind { binding, .. } = stmt {
                    out.insert(*binding);
                }
            }
        }
        Expr::Case { arms, .. } => {
            for arm in arms {
                collect_pattern_bindings(body, arm.pat, out);
            }
        }
        Expr::For { pat, .. } | Expr::In { pat, .. } => collect_pattern_bindings(body, *pat, out),
        Expr::Closure { params, .. } => {
            for p in params.iter().flatten() {
                collect_pattern_bindings(body, p.pat, out);
            }
        }
        _ => {}
    });
}

/// Bindings used inside `expr`, and those it mutates (assigns or passes in `mut` mode).
fn collect_uses(
    body: &Body,
    typed: &TypedBody,
    expr: ExprId,
    used: &mut Vec<BindingId>,
    mutated: &mut HashSet<BindingId>,
    fns: &HashMap<DefId, loom_check::FnSig>,
) {
    let root_binding = |mut e: ExprId| -> Option<BindingId> {
        loop {
            match body.expr(e) {
                Expr::Name(_) => {
                    return match typed.exprs.get(&e) {
                        Some(ExprInfo::Local(b)) => Some(typed.canonical(*b)),
                        _ => None,
                    };
                }
                Expr::Field { base, .. }
                | Expr::TupleField { base, .. }
                | Expr::Index { base, .. } => e = *base,
                _ => return None,
            }
        }
    };
    walk_exprs(body, expr, &mut |e| match body.expr(e) {
        Expr::Name(_) => {
            if let Some(ExprInfo::Local(b)) = typed.exprs.get(&e) {
                used.push(typed.canonical(*b));
            }
        }
        Expr::Block { stmts } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Assign { place, .. } => {
                        if let Some(b) = root_binding(*place) {
                            mutated.insert(b);
                        }
                    }
                    Stmt::Bind { binding, .. } => {
                        if let Some(&existing) = typed.assignments.get(binding) {
                            let existing = typed.canonical(existing);
                            used.push(existing);
                            mutated.insert(existing);
                        }
                    }
                    Stmt::Expr(_) => {}
                }
            }
        }
        Expr::Call { args, .. } | Expr::MethodCall { args, .. } => {
            if let Some(ExprInfo::Call {
                target,
                receiver,
                args: sources,
            }) = typed.exprs.get(&e)
            {
                let sig = match target {
                    CallTarget::Fn { def, .. } | CallTarget::Trait { method: def, .. } => {
                        fns.get(def)
                    }
                    _ => None,
                };
                if let Some(sig) = sig {
                    if sig.receiver == Some(Mode::Mut)
                        && let Some(b) = receiver.and_then(root_binding)
                    {
                        mutated.insert(b);
                    }
                    for (p, source) in sources.iter().enumerate() {
                        if let (ArgSource::Arg(i), Some((Mode::Mut, ..))) =
                            (source, sig.params.get(p))
                            && let Some(b) = root_binding(args[*i].value)
                        {
                            mutated.insert(b);
                        }
                    }
                }
            }
        }
        _ => {}
    });
}

/// Calls `f` on `expr` and every expression inside it.
fn walk_exprs(body: &Body, expr: ExprId, f: &mut impl FnMut(ExprId)) {
    f(expr);
    let mut children = Vec::new();
    match body.expr(expr) {
        Expr::Field { base, .. }
        | Expr::TupleField { base, .. }
        | Expr::Try(base)
        | Expr::Unary { operand: base, .. }
        | Expr::Loop(base) => children.push(*base),
        Expr::Call { callee, args } => {
            children.push(*callee);
            children.extend(args.iter().map(|a| a.value));
        }
        Expr::MethodCall { receiver, args, .. } => {
            children.push(*receiver);
            children.extend(args.iter().map(|a| a.value));
        }
        Expr::Index { base, indices, .. } => {
            children.push(*base);
            children.extend(indices.iter().copied());
        }
        Expr::Binary { lhs, rhs, .. }
        | Expr::Range {
            start: lhs,
            end: rhs,
            ..
        } => children.extend([*lhs, *rhs]),
        Expr::In { expr: e, .. } => children.push(*e),
        Expr::Interpolate(parts) => children.extend(parts.iter().filter_map(|p| match p {
            InterpolationPart::Expr(e) => Some(*e),
            InterpolationPart::Text(_) => None,
        })),
        Expr::Tuple(es) | Expr::Array(es) => children.extend(es.iter().copied()),
        Expr::Map(entries) => {
            for &(k, v) in entries {
                children.extend([k, v]);
            }
        }
        Expr::Closure { body: b, .. } => children.push(*b),
        Expr::Block { stmts } => {
            for stmt in stmts {
                match stmt {
                    Stmt::Bind { value, .. } => children.push(*value),
                    Stmt::Assign { place, value, .. } => children.extend([*place, *value]),
                    Stmt::Expr(e) => children.push(*e),
                }
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            children.extend([*cond, *then_branch]);
            children.extend(else_branch.iter().copied());
        }
        Expr::Case {
            scrutinee,
            arms,
            else_branch,
        } => {
            children.push(*scrutinee);
            for arm in arms {
                children.extend(arm.guard.iter().copied());
                children.push(arm.body);
            }
            children.extend(else_branch.iter().copied());
        }
        Expr::While { cond, body: b } => children.extend([*cond, *b]),
        Expr::For { iter, body: b, .. } => children.extend([*iter, *b]),
        Expr::Return(v) | Expr::Break(v) => children.extend(v.iter().copied()),
        _ => {}
    }
    for child in children {
        walk_exprs(body, child, f);
    }
}
