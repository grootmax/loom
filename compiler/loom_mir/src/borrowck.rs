//! The ownership and borrow checker (ADR 0020 §3–§4, spec §8).
//!
//! It runs on MIR in four steps:
//!
//! 1. **Moves.** A forward dataflow of maybe-moved places reports a use after move (E0500)
//!    and moves out of borrowed places (E0501).
//! 2. **Loans.** Every `Rvalue::Ref` creates a loan on its place. A local carries the
//!    loans of the values assigned to it when its type is borrowed (spec §8.3 rule 1), so
//!    a call's result carries the loans of its borrowed arguments, which is the elision
//!    rule. Values carrying loans may not flow into non-borrowed destinations (E0503).
//! 3. **Liveness.** A loan is live while a local carrying it may still be used (NLL).
//! 4. **Conflicts.** Writing, moving or mutably borrowing a place under a live loan, or
//!    reading it under a live mutable loan, is E0502.

use crate::{
    AggregateKind, Callee, Local, MirBody, Operand, Place, Proj, Rvalue, StatementKind,
    TerminatorKind,
};
use loom_check::{Checked, Env, Items, Solver, TraitRef};
use loom_diagnostics::{Diagnostic, codes};
use loom_hir::{DefKind, Hir};
use loom_span::Span;
use loom_types::{InferTable, Ty, TyKind};
use std::collections::{HashMap, HashSet};

/// Checks every MIR body of the program.
#[must_use]
#[allow(clippy::similar_names, reason = "names mirror the MIR terms")]
pub fn check_program(hir: &Hir, checked: &Checked, mir: &crate::Mir) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for body in &mir.bodies {
        let generics = checked.items.generics_of(body.owner);
        let mut table = InferTable::new();
        let env = Solver {
            hir,
            items: &checked.items,
            interner: &checked.interner,
            table: &mut table,
        }
        .elaborate(&generics.predicates);
        let mut checker = Checker {
            hir,
            items: &checked.items,
            interner: &checked.interner,
            env,
            body,
            diagnostics: Vec::new(),
        };
        checker.run();
        diagnostics.extend(checker.diagnostics);
    }
    diagnostics.sort_by_key(|d| (d.primary.span.file, d.primary.span.range.start()));
    diagnostics.dedup_by_key(|d| (d.primary.span, d.id));
    diagnostics
}

/// A program point: a block and a statement index (the terminator is `statements.len()`).
type Point = (usize, usize);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Loan {
    place: Place,
    mutable: bool,
    span: Span,
}

struct Checker<'a> {
    hir: &'a Hir,
    items: &'a Items,
    interner: &'a loom_types::Interner,
    env: Env,
    body: &'a MirBody,
    diagnostics: Vec<Diagnostic>,
}

/// What one statement or terminator does, for the analyses.
#[derive(Default)]
struct Effects {
    /// Places read (operands, borrowed places, discriminants).
    reads: Vec<Place>,
    /// Places moved.
    moves: Vec<Place>,
    /// Places written (assignment destinations).
    writes: Vec<Place>,
    /// Places borrowed: (place, mutable).
    borrows: Vec<(Place, bool)>,
    /// Places dropped.
    drops: Vec<Place>,
}

impl Checker<'_> {
    fn run(&mut self) {
        let effects = self.collect_effects();
        self.check_moves(&effects);
        let loans = self.compute_loans();
        self.check_escapes(&loans);
        let live = self.liveness();
        self.check_conflicts(&effects, &loans, &live);
    }

    fn ty(&self, place: &Place) -> Ty {
        crate::place_ty(self.hir, self.items, self.interner, self.body, place)
    }

    fn is_copy(&self, ty: Ty) -> bool {
        match self.interner.kind(ty) {
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
                let Some(copy) = self.items.lang.get("Copy") else {
                    return false;
                };
                let mut table = InferTable::new();
                Solver {
                    hir: self.hir,
                    items: self.items,
                    interner: self.interner,
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

    /// Whether values of `ty` may carry borrows (spec §8.3 rule 1). Unknown types
    /// (generic parameters) count as borrowed, so generic code is not rejected.
    fn is_borrowed(&self, ty: Ty) -> bool {
        let interner = self.interner;
        interner.any(ty, &|kind| match kind {
            TyKind::Ref { .. }
            | TyKind::Param(_)
            | TyKind::Projection { .. }
            | TyKind::Infer(..)
            | TyKind::Error => true,
            TyKind::Adt(def, _) => self.hir.def(*def).attr("view").is_some(),
            _ => false,
        })
    }

    fn statement_span(&self, (block, index): Point) -> Span {
        let block = &self.body.blocks[block];
        match block.statements.get(index) {
            Some(s) => s.span,
            None => block.terminator.as_ref().map_or(self.body.span, |t| t.span),
        }
    }

    fn collect_effects(&self) -> HashMap<Point, Effects> {
        let mut out = HashMap::new();
        for (b, block) in self.body.blocks.iter().enumerate() {
            for (i, stmt) in block.statements.iter().enumerate() {
                let mut e = Effects::default();
                if let StatementKind::Assign(place, rvalue) = &stmt.kind {
                    rvalue_effects(rvalue, &mut e);
                    e.writes.push(place.clone());
                    index_reads(place, &mut e);
                }
                out.insert((b, i), e);
            }
            let mut e = Effects::default();
            if let Some(t) = &block.terminator {
                match &t.kind {
                    TerminatorKind::SwitchInt { discr, .. } => operand_effects(discr, &mut e),
                    TerminatorKind::Call {
                        callee, args, dest, ..
                    } => {
                        if let Callee::Value(op) = callee
                            && let Operand::Copy(p) | Operand::Move(p) = op
                        {
                            e.reads.push(p.clone());
                        }
                        for a in args {
                            operand_effects(a, &mut e);
                        }
                        e.writes.push(dest.clone());
                    }
                    TerminatorKind::Drop { place, .. } => e.drops.push(place.clone()),
                    _ => {}
                }
            }
            out.insert((b, block.statements.len()), e);
        }
        out
    }

    // ----- 1. moves --------------------------------------------------------------------------

    fn check_moves(&mut self, effects: &HashMap<Point, Effects>) {
        let blocks = self.body.blocks.len();
        // Maybe-moved places at the start of each block, with where they were moved.
        let mut entry: Vec<Option<HashMap<Place, Span>>> = vec![None; blocks];
        entry[0] = Some(HashMap::new());
        let mut worklist = vec![0usize];
        let mut reported: HashSet<Span> = HashSet::new();
        let mut errors: Vec<Diagnostic> = Vec::new();
        let mut iterations = 0;
        while let Some(b) = worklist.pop() {
            iterations += 1;
            if iterations > 100_000 {
                break;
            }
            let mut state = entry[b].clone().unwrap_or_default();
            let count = self.body.blocks[b].statements.len();
            for i in 0..=count {
                let point = (b, i);
                let Some(e) = effects.get(&point) else {
                    continue;
                };
                let span = self.statement_span(point);
                for used in e
                    .reads
                    .iter()
                    .chain(&e.moves)
                    .chain(e.borrows.iter().map(|(p, _)| p))
                {
                    if let Some((moved, at)) = state.iter().find(|(m, _)| m.overlaps(used)) {
                        let message = if moved.is_prefix_of(used) || moved == used {
                            "use of a moved value".to_owned()
                        } else {
                            "use of a partly moved value".to_owned()
                        };
                        let at = *at;
                        let _ = moved;
                        if reported.insert(span) {
                            errors.push(
                                Diagnostic::error(codes::USE_AFTER_MOVE, span, message)
                                    .with_label("used here after the move")
                                    .with_secondary(at, "the value was moved here")
                                    .with_help("use the value before moving it, or move a `.clone` instead"),
                            );
                        }
                    }
                }
                for moved in &e.moves {
                    if (moved.is_behind_ref()
                        || moved.proj.iter().any(|p| matches!(p, Proj::Index(_))))
                        && !self.is_copy(self.ty(moved))
                        && reported.insert(span)
                    {
                        let what = if moved.proj.iter().any(|p| matches!(p, Proj::Index(_))) {
                            "cannot move a value out of a collection element"
                        } else {
                            "cannot move a value out of a borrow"
                        };
                        errors.push(
                            Diagnostic::error(codes::MOVE_OUT_OF_BORROW, span, what)
                                .with_label("this value is only borrowed here")
                                .with_help(
                                    "move a copy with `.clone`, or take the parameter with `own`",
                                ),
                        );
                    }
                    if !moved.is_behind_ref() {
                        state.entry(moved.clone()).or_insert(span);
                    }
                }
                for written in &e.writes {
                    state.retain(|m, _| !written.is_prefix_of(m));
                }
            }
            for succ in self.body.blocks[b].successors() {
                let s = succ.index();
                let merged = match &entry[s] {
                    None => Some(state.clone()),
                    Some(existing) => {
                        let mut union = existing.clone();
                        let before = union.len();
                        for (k, v) in &state {
                            union.entry(k.clone()).or_insert(*v);
                        }
                        (union.len() != before).then_some(union)
                    }
                };
                if let Some(m) = merged {
                    entry[s] = Some(m);
                    worklist.push(s);
                }
            }
        }
        self.diagnostics.extend(errors);
    }

    // ----- 2. loans --------------------------------------------------------------------------

    /// The loans each local may carry (flow-insensitive).
    fn compute_loans(&self) -> Vec<HashSet<Loan>> {
        let mut loans: Vec<HashSet<Loan>> = vec![HashSet::new(); self.body.locals.len()];
        // The loan each `ref` temporary was created with (as opposed to loans it inherits).
        let mut direct: HashMap<Local, Loan> = HashMap::new();
        for block in &self.body.blocks {
            for stmt in &block.statements {
                if let StatementKind::Assign(dest, Rvalue::Ref { mutable, place }) = &stmt.kind
                    && dest.proj.is_empty()
                {
                    direct.insert(
                        dest.local,
                        Loan {
                            place: place.clone(),
                            mutable: *mutable,
                            span: stmt.span,
                        },
                    );
                }
            }
        }
        let mut changed = true;
        let mut rounds = 0;
        while changed && rounds < 50 {
            changed = false;
            rounds += 1;
            for block in &self.body.blocks {
                for stmt in &block.statements {
                    let StatementKind::Assign(dest, rvalue) = &stmt.kind else {
                        continue;
                    };
                    let mut incoming: HashSet<Loan> = HashSet::new();
                    match rvalue {
                        Rvalue::Ref { mutable, place } => {
                            incoming.insert(Loan {
                                place: place.clone(),
                                mutable: *mutable,
                                span: stmt.span,
                            });
                            incoming.extend(self.loans_of_place(place, &loans));
                        }
                        Rvalue::Use(op) | Rvalue::Unsize(op, _) => {
                            incoming.extend(self.operand_loans(op, &loans));
                        }
                        Rvalue::Aggregate(_, ops) => {
                            for op in ops {
                                incoming.extend(self.operand_loans(op, &loans));
                            }
                        }
                        _ => {}
                    }
                    let dest_ty = self.ty(dest);
                    let carries = self.is_borrowed(dest_ty)
                        || matches!(self.interner.kind(dest_ty), TyKind::Callable { .. });
                    if carries && !incoming.is_empty() {
                        let target = &mut loans[dest.local.index()];
                        let before = target.len();
                        target.extend(incoming);
                        changed |= target.len() != before;
                    }
                }
                if let Some(t) = &block.terminator
                    && let TerminatorKind::Call {
                        callee, args, dest, ..
                    } = &t.kind
                {
                    let dest_ty = self.ty(dest);
                    if self.is_borrowed(dest_ty)
                        || matches!(self.interner.kind(dest_ty), TyKind::Callable { .. })
                    {
                        let incoming = self.call_result_loans(callee, args, &loans, &direct);
                        let target = &mut loans[dest.local.index()];
                        let before = target.len();
                        target.extend(incoming);
                        changed |= target.len() != before;
                    }
                }
            }
        }
        loans
    }

    /// The loans a call's result carries (spec §8.3 rule 3): the borrow of the elided
    /// source if the callee's declared return type contains `ref`, and the loans its
    /// arguments already carried if that return type could hold them.
    fn call_result_loans(
        &self,
        callee: &Callee,
        args: &[Operand],
        loans: &[HashSet<Loan>],
        direct: &HashMap<Local, Loan>,
    ) -> HashSet<Loan> {
        let mut out = HashSet::new();
        let sig = match callee {
            Callee::Fn { def, .. } | Callee::Trait { method: def, .. } => self.items.fns.get(def),
            Callee::Builtin("map_index") => {
                if let Some(a) = args.first() {
                    out.extend(self.operand_loans(a, loans));
                }
                return out;
            }
            Callee::Builtin(_) | Callee::Default { .. } => return out,
            Callee::Value(callee) => {
                // A closure cannot return a borrow of its arguments (they are borrowed
                // only for the call); its result may hold what the closure captured.
                let _ = args;
                out.extend(self.operand_loans(callee, loans));
                return out;
            }
        };
        let Some(sig) = sig else { return out };
        let interner = self.interner;
        let ret_has_ref = interner.any(sig.ret, &|k| matches!(k, TyKind::Ref { .. }))
            || matches!(interner.kind(sig.ret), TyKind::Adt(def, _) if self.hir.def(def).attr("view").is_some());
        let ret_may_hold = ret_has_ref
            || interner.any(sig.ret, &|k| {
                matches!(k, TyKind::Param(_) | TyKind::Projection { .. })
            });
        let mut modes: Vec<loom_hir::Mode> = Vec::new();
        if let Some(r) = sig.receiver {
            modes.push(r);
        }
        modes.extend(sig.params.iter().map(|p| p.0));
        // The elided source: the receiver, or the only borrowed non-Copy parameter.
        let elided = if sig.receiver.is_some() {
            Some(0)
        } else {
            let borrowed: Vec<usize> = sig
                .params
                .iter()
                .enumerate()
                .filter(|(_, p)| p.0 != loom_hir::Mode::Own && !self.is_copy(p.2))
                .map(|(i, _)| i)
                .collect();
            (borrowed.len() == 1).then(|| borrowed[0])
        };
        for (i, arg) in args.iter().enumerate() {
            let (Operand::Copy(p) | Operand::Move(p)) = arg else {
                continue;
            };
            let arg_loans = &loans[p.local.index()];
            let mode = modes.get(i).copied().unwrap_or(loom_hir::Mode::Own);
            if mode == loom_hir::Mode::Own {
                if ret_may_hold {
                    out.extend(self.operand_loans(arg, loans));
                }
                continue;
            }
            let own_loan = direct.get(&p.local);
            for loan in arg_loans {
                let is_call_borrow = own_loan == Some(loan);
                if (is_call_borrow && ret_has_ref && elided == Some(i))
                    || (!is_call_borrow && ret_may_hold)
                {
                    out.insert(loan.clone());
                }
            }
            if ret_has_ref
                && elided == Some(i)
                && let Some(loan) = own_loan
            {
                out.insert(loan.clone());
            }
        }
        out
    }

    /// Loans carried by the value read through `place`: those of its root local if the
    /// read goes through a borrow (reborrowing keeps the original loans alive).
    #[allow(
        clippy::unused_self,
        reason = "a method for uniformity with its siblings"
    )]
    fn loans_of_place(&self, place: &Place, loans: &[HashSet<Loan>]) -> HashSet<Loan> {
        if place.is_behind_ref() {
            loans[place.local.index()].clone()
        } else {
            HashSet::new()
        }
    }

    fn operand_loans(&self, op: &Operand, loans: &[HashSet<Loan>]) -> HashSet<Loan> {
        match op {
            Operand::Copy(p) | Operand::Move(p) => {
                let ty = self.ty(p);
                if self.is_borrowed(ty)
                    || matches!(self.interner.kind(ty), TyKind::Callable { .. })
                    || p.is_behind_ref()
                {
                    loans[p.local.index()].clone()
                } else {
                    HashSet::new()
                }
            }
            Operand::Const(..) => HashSet::new(),
        }
    }

    // ----- escapes ---------------------------------------------------------------------------

    fn check_escapes(&mut self, loans: &[HashSet<Loan>]) {
        let mut errors = Vec::new();
        for block in &self.body.blocks {
            for stmt in &block.statements {
                let StatementKind::Assign(dest, rvalue) = &stmt.kind else {
                    continue;
                };
                let ops: Vec<&Operand> = match rvalue {
                    Rvalue::Aggregate(AggregateKind::Closure(_), _) => Vec::new(),
                    Rvalue::Aggregate(_, ops) => ops.iter().collect(),
                    Rvalue::Use(op) => vec![op],
                    _ => Vec::new(),
                };
                let dest_ty = self.ty(dest);
                if self.is_borrowed(dest_ty) {
                    continue;
                }
                for op in ops {
                    if let Operand::Copy(p) | Operand::Move(p) = op {
                        let ty = self.ty(p);
                        let carries = !self.operand_loans(op, loans).is_empty();
                        if carries
                            && matches!(self.interner.kind(ty), TyKind::Callable { .. })
                            && matches!(self.interner.kind(dest_ty), TyKind::Callable { .. })
                            && dest.proj.is_empty()
                        {
                            // A closure moved between locals keeps its loans.
                            continue;
                        }
                        if carries
                            && (self.is_borrowed(ty)
                                || matches!(self.interner.kind(ty), TyKind::Callable { .. }))
                        {
                            errors.push(
                                Diagnostic::error(
                                    codes::BORROW_ESCAPES,
                                    stmt.span,
                                    "a borrowed value cannot be stored here",
                                )
                                .with_label(
                                    "this value borrows from something that may not outlive it",
                                )
                                .with_help(
                                    "store an owned value instead (for example with `.clone`)",
                                ),
                            );
                        }
                    }
                }
            }
            if let Some(t) = &block.terminator
                && let TerminatorKind::Call {
                    callee: Callee::Fn { def, .. },
                    args,
                    ..
                } = &t.kind
                && let Some(sig) = self.items.fns.get(def)
            {
                // A closure that captures by reference passed to `own` (spec §8.4 rule 3).
                let offset = usize::from(sig.receiver.is_some());
                for (i, (mode, _, ty, _)) in sig.params.iter().enumerate() {
                    if *mode != loom_hir::Mode::Own || self.is_borrowed(*ty) {
                        continue;
                    }
                    if let Some(op) = args.get(i + offset)
                        && !self.operand_loans(op, loans).is_empty()
                    {
                        errors.push(
                            Diagnostic::error(codes::BORROW_ESCAPES, t.span, "a borrowed value cannot be passed to an `own` parameter")
                                .with_label("this argument borrows from something that may not outlive the call")
                                .with_help("pass an owned value, or a block written directly in the call"),
                        );
                    }
                }
            }
        }
        // Returned borrows must come from the elided source (spec §8.3 rule 3); a closure
        // may return borrows of its captures only.
        let ret_loans = &loans[0];
        let closure = matches!(self.body.source, crate::MirSource::Closure(..));
        for loan in ret_loans {
            let local = self.body.local(loan.place.local);
            let from_argument = loan.place.local.index() >= 1
                && loan.place.local.index() <= self.body.arg_count
                && loan.place.is_behind_ref()
                && (!closure || matches!(local.kind, crate::LocalKind::ClosureEnv));
            if !from_argument && !matches!(local.kind, crate::LocalKind::Return) {
                errors.push(
                    Diagnostic::error(
                        codes::BORROW_ESCAPES,
                        loan.span,
                        "this function returns a borrow of a value it owns",
                    )
                    .with_label("the borrowed value is dropped when the function returns")
                    .with_help("return an owned value instead"),
                );
            }
        }
        self.diagnostics.extend(errors);
    }

    // ----- 3. liveness -----------------------------------------------------------------------

    /// The locals live before each point.
    fn liveness(&self) -> HashMap<Point, HashSet<Local>> {
        let blocks = &self.body.blocks;
        let mut live_in: Vec<HashSet<Local>> = vec![HashSet::new(); blocks.len()];
        let mut result: HashMap<Point, HashSet<Local>> = HashMap::new();
        let mut changed = true;
        let mut rounds = 0;
        while changed && rounds < 200 {
            changed = false;
            rounds += 1;
            for b in (0..blocks.len()).rev() {
                let mut live: HashSet<Local> = HashSet::new();
                for succ in blocks[b].successors() {
                    live.extend(live_in[succ.index()].iter().copied());
                }
                let count = blocks[b].statements.len();
                if let Some(t) = &blocks[b].terminator {
                    match &t.kind {
                        TerminatorKind::Call {
                            callee, args, dest, ..
                        } => {
                            if dest.proj.is_empty() {
                                live.remove(&dest.local);
                            } else {
                                live.insert(dest.local);
                            }
                            for a in args {
                                operand_locals(a, &mut live);
                            }
                            if let Callee::Value(op) = callee {
                                operand_locals(op, &mut live);
                            }
                        }
                        TerminatorKind::SwitchInt { discr, .. } => operand_locals(discr, &mut live),
                        TerminatorKind::Return => {
                            live.insert(Local(0));
                        }
                        TerminatorKind::Drop { place, .. } => {
                            // Dropping a value that holds borrows does not use them.
                            let _ = place;
                        }
                        _ => {}
                    }
                }
                result.insert((b, count), live.clone());
                for i in (0..count).rev() {
                    if let StatementKind::Assign(dest, rvalue) = &blocks[b].statements[i].kind {
                        if dest.proj.is_empty() {
                            live.remove(&dest.local);
                        } else {
                            live.insert(dest.local);
                            place_locals(dest, &mut live);
                        }
                        rvalue_locals(rvalue, &mut live);
                    }
                    result.insert((b, i), live.clone());
                }
                if live != live_in[b] {
                    live_in[b] = live;
                    changed = true;
                }
            }
        }
        result
    }

    // ----- 4. conflicts ----------------------------------------------------------------------

    fn check_conflicts(
        &mut self,
        effects: &HashMap<Point, Effects>,
        loans: &[HashSet<Loan>],
        live: &HashMap<Point, HashSet<Local>>,
    ) {
        let mut errors = Vec::new();
        let mut reported: HashSet<Span> = HashSet::new();
        for (b, block) in self.body.blocks.iter().enumerate() {
            for i in 0..=block.statements.len() {
                let point = (b, i);
                let Some(e) = effects.get(&point) else {
                    continue;
                };
                // Loans live after this point (carried by locals used later).
                let after = if i < block.statements.len() {
                    live.get(&(b, i + 1)).cloned().unwrap_or_default()
                } else {
                    let mut all = HashSet::new();
                    for succ in block.successors() {
                        all.extend(live.get(&(succ.index(), 0)).cloned().unwrap_or_default());
                    }
                    all
                };
                let before = live.get(&point).cloned().unwrap_or_default();
                // A loan is in force at a point if a local carrying it is live just before
                // and just after (it is still needed across this point).
                let mut active: Vec<(&Loan, Local)> = Vec::new();
                for (index, set) in loans.iter().enumerate() {
                    let local = Local(u32::try_from(index).unwrap_or(0));
                    if before.contains(&local) && after.contains(&local) {
                        active.extend(set.iter().map(|l| (l, local)));
                    }
                }
                if active.is_empty() {
                    continue;
                }
                let span = self.statement_span(point);
                let mut conflict = |place: &Place, kind: Access| {
                    for (loan, carrier) in &active {
                        if place.local == *carrier {
                            continue;
                        }
                        if !loan.place.overlaps(place) {
                            continue;
                        }
                        let bad = match kind {
                            Access::Read => loan.mutable,
                            Access::Write | Access::Move | Access::MutBorrow => true,
                        };
                        if bad && loan.span != span && reported.insert(span) {
                            let what = match kind {
                                Access::Read => {
                                    "cannot use this value while it is borrowed mutably"
                                }
                                Access::Write => "cannot assign to this value while it is borrowed",
                                Access::Move => "cannot move this value while it is borrowed",
                                Access::MutBorrow => {
                                    "cannot borrow this value mutably while it is borrowed"
                                }
                            };
                            errors.push(
                                Diagnostic::error(codes::BORROW_CONFLICT, span, what)
                                    .with_label("conflicting use")
                                    .with_secondary(
                                        loan.span,
                                        "the borrow starts here and is used later",
                                    )
                                    .with_help("finish using the borrow first, or work on a copy"),
                            );
                        }
                    }
                };
                for p in &e.writes {
                    conflict(p, Access::Write);
                }
                for p in &e.moves {
                    conflict(p, Access::Move);
                }
                for (p, mutable) in &e.borrows {
                    conflict(
                        p,
                        if *mutable {
                            Access::MutBorrow
                        } else {
                            Access::Read
                        },
                    );
                }
                for p in &e.reads {
                    conflict(p, Access::Read);
                }
            }
        }
        self.diagnostics.extend(errors);
    }
}

#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
    Move,
    MutBorrow,
}

fn operand_effects(op: &Operand, e: &mut Effects) {
    match op {
        Operand::Copy(p) => {
            e.reads.push(p.clone());
            index_reads(p, e);
        }
        Operand::Move(p) => {
            e.moves.push(p.clone());
            index_reads(p, e);
        }
        Operand::Const(..) => {}
    }
}

fn index_reads(place: &Place, e: &mut Effects) {
    for step in &place.proj {
        if let Proj::Index(l) = step {
            e.reads.push(Place::local(*l));
        }
    }
}

fn rvalue_effects(rvalue: &Rvalue, e: &mut Effects) {
    match rvalue {
        Rvalue::Use(op) | Rvalue::Unary(_, op) | Rvalue::Unsize(op, _) => operand_effects(op, e),
        Rvalue::Binary(_, a, b) => {
            operand_effects(a, e);
            operand_effects(b, e);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                operand_effects(op, e);
            }
        }
        Rvalue::Ref { mutable, place } => {
            e.borrows.push((place.clone(), *mutable));
            index_reads(place, e);
        }
        Rvalue::Discriminant(place) => e.reads.push(place.clone()),
    }
}

fn operand_locals(op: &Operand, live: &mut HashSet<Local>) {
    if let Operand::Copy(p) | Operand::Move(p) = op {
        place_locals(p, live);
    }
}

fn place_locals(place: &Place, live: &mut HashSet<Local>) {
    live.insert(place.local);
    for step in &place.proj {
        if let Proj::Index(l) = step {
            live.insert(*l);
        }
    }
}

fn rvalue_locals(rvalue: &Rvalue, live: &mut HashSet<Local>) {
    match rvalue {
        Rvalue::Use(op) | Rvalue::Unary(_, op) | Rvalue::Unsize(op, _) => operand_locals(op, live),
        Rvalue::Binary(_, a, b) => {
            operand_locals(a, live);
            operand_locals(b, live);
        }
        Rvalue::Aggregate(_, ops) => {
            for op in ops {
                operand_locals(op, live);
            }
        }
        Rvalue::Ref { place, .. } | Rvalue::Discriminant(place) => place_locals(place, live),
    }
}

/// Whether `def` is a function in an `extern` block (used by callers that skip them).
#[must_use]
pub fn is_extern(hir: &Hir, def: loom_hir::DefId) -> bool {
    matches!(&hir.def(def).kind, DefKind::Fn(f) if f.extern_abi.is_some())
}
