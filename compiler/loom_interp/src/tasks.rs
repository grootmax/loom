//! The concurrency intrinsics (spec §11, ADR 0022 §9): tasks run on the runtime's
//! deterministic scheduler, in this thread.

use crate::eval::ArgVal;
use crate::value::Handle;
use crate::{Eval, Flow, Interp, Place, Value};
use loom_hir::DefId;
use loom_resolve::TyParamId;
use loom_runtime::sched::{self, Lock, chan::Chan, chan::SendError};
use loom_span::Span;
use loom_types::{Subst, Ty};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The intrinsics this module implements.
const NAMES: &[&str] = &[
    "scope_run",
    "scope_spawn",
    "scope_cancel",
    "scope_cancelled",
    "task_join",
    "task_detach",
    "task_yield",
    "chan_new",
    "chan_send",
    "chan_recv",
    "chan_try_recv",
    "chan_close",
    "time_sleep",
    "time_now",
    "mutex_new",
    "mutex_lock",
    "atomic_new",
    "atomic_op",
    "shared_new",
    "shared_clone",
    "shared_get",
    "shared_count",
];

/// Whether `name` is a concurrency intrinsic.
pub(crate) fn is_task_intrinsic(name: &str) -> bool {
    NAMES.contains(&name)
}

fn handle(h: Handle) -> Value {
    Value::Handle(Rc::new(h))
}

impl Interp<'_> {
    fn type_param(&self, def: DefId, subst: &Subst, index: u32) -> Ty {
        subst
            .get(TyParamId { owner: def, index })
            .unwrap_or_else(|| self.interner().error())
    }

    fn option(&self, elem: Ty, value: Option<Value>) -> Value {
        match value {
            Some(v) => self.lang_variant("Option", "Some", vec![elem], vec![v]),
            None => self.lang_variant("Option", "None", vec![elem], Vec::new()),
        }
    }

    /// The interpreter for code that runs in another task.
    fn task_self(&self, span: Option<Span>) -> Eval<&'static Interp<'static>> {
        match (self.this.get(), sched::current()) {
            (Some(me), Some(_)) => Ok(me),
            _ => Err(Flow::Panic(
                "internal error: tasks need the scheduled interpreter".to_owned(),
                span,
            )),
        }
    }

    #[allow(clippy::too_many_lines, reason = "one arm per intrinsic")]
    pub(crate) fn task_intrinsic(
        &self,
        def: DefId,
        name: &str,
        subst: &Subst,
        args: &[ArgVal],
        span: Option<Span>,
    ) -> Eval<Value> {
        let arg = |i: usize| args.get(i).map_or(Value::Unit, ArgVal::value);
        let panic = |message: &str| Err(Flow::Panic(message.to_owned(), span));
        let in_task = sched::current().is_some();
        if !in_task && name != "time_now" {
            return panic("internal error: tasks need the scheduled interpreter");
        }
        let get = |i: usize| match arg(i) {
            Value::Handle(h) => Some(h),
            _ => None,
        };
        match name {
            "scope_run" => {
                let deadline = arg(0).as_int();
                let body = arg(1);
                let deadline = u64::try_from(deadline).ok();
                let (scope, previous) = sched::enter_scope(deadline);
                let value = handle(Handle::Scope(std::sync::Arc::clone(&scope)));
                let result = self.call_value(&body, vec![ArgVal::Value(value)], span);
                sched::exit_scope(&scope, previous);
                result
            }
            "scope_spawn" => {
                let Some(h) = get(0) else {
                    return panic("internal error: spawn without a scope");
                };
                let Handle::Scope(scope) = &*h else {
                    return panic("internal error: spawn without a scope");
                };
                let work = arg(1);
                let me = self.task_self(span)?;
                let result: Rc<RefCell<Option<Value>>> = Rc::new(RefCell::new(None));
                let cell = Rc::clone(&result);
                let task = sched::spawn_in(
                    scope,
                    Box::new(move || match me.call_value(&work, Vec::new(), span) {
                        Ok(v) => *cell.borrow_mut() = Some(v),
                        Err(Flow::Panic(message, at)) => me.task_panicked(message, at),
                        Err(_) => {}
                    }),
                );
                Ok(handle(Handle::Task(task, result)))
            }
            "scope_cancel" | "scope_cancelled" => match get(0).as_deref() {
                Some(Handle::Scope(scope)) => {
                    if name == "scope_cancel" {
                        scope.cancel();
                        Ok(Value::Unit)
                    } else {
                        Ok(Value::Bool(scope.is_cancelled()))
                    }
                }
                _ => panic("internal error: not a scope"),
            },
            "task_join" => match get(0).as_deref() {
                Some(Handle::Task(task, result)) => {
                    sched::join(task);
                    Ok(result.borrow_mut().take().unwrap_or(Value::Unit))
                }
                _ => panic("internal error: not a task"),
            },
            "task_detach" => {
                let work = arg(0);
                let me = self.task_self(span)?;
                sched::spawn_detached(Box::new(move || {
                    if let Err(Flow::Panic(message, at)) = me.call_value(&work, Vec::new(), span) {
                        me.task_panicked(message, at);
                    }
                }));
                Ok(Value::Unit)
            }
            "task_yield" => {
                sched::yield_now();
                Ok(Value::Unit)
            }
            "chan_new" => {
                let capacity = usize::try_from(arg(0).as_int()).unwrap_or(0);
                Ok(handle(Handle::Chan(Chan::new(capacity))))
            }
            "chan_send" => match get(0).as_deref() {
                Some(Handle::Chan(ch)) => match ch.send(arg(1)) {
                    Ok(()) => Ok(Value::Bool(true)),
                    Err(SendError::Cancelled(_)) => Ok(Value::Bool(false)),
                    Err(SendError::Closed(_)) => panic("send on a closed channel"),
                },
                _ => panic("internal error: not a channel"),
            },
            "chan_recv" | "chan_try_recv" => match get(0).as_deref() {
                Some(Handle::Chan(ch)) => {
                    let value = if name == "chan_recv" { ch.recv() } else { ch.try_recv() };
                    Ok(self.option(self.type_param(def, subst, 0), value))
                }
                _ => panic("internal error: not a channel"),
            },
            "chan_close" => match get(0).as_deref() {
                Some(Handle::Chan(ch)) => {
                    ch.close();
                    Ok(Value::Unit)
                }
                _ => panic("internal error: not a channel"),
            },
            "time_sleep" => {
                let ns = u64::try_from(arg(0).as_int()).unwrap_or(0);
                Ok(Value::Bool(sched::sleep(ns)))
            }
            "time_now" => {
                let now = sched::sched().map_or(0, |s| s.now());
                Ok(Value::int(i128::from(now)))
            }
            "mutex_new" => Ok(handle(Handle::Mutex(Place::new(arg(0)), Lock::new()))),
            "mutex_lock" => match get(0).as_deref() {
                Some(Handle::Mutex(place, lock)) => {
                    lock.acquire();
                    let result = self.call_value(&arg(1), vec![ArgVal::Place(place.clone())], span);
                    lock.release();
                    result
                }
                _ => panic("internal error: not a mutex"),
            },
            "atomic_new" => {
                let v = i64::try_from(arg(0).as_int()).unwrap_or(0);
                Ok(handle(Handle::Atomic(Cell::new(v))))
            }
            "atomic_op" => match get(0).as_deref() {
                Some(Handle::Atomic(cell)) => {
                    let (a, b) = (
                        i64::try_from(arg(2).as_int()).unwrap_or(0),
                        i64::try_from(arg(3).as_int()).unwrap_or(0),
                    );
                    let old = cell.get();
                    let result = match arg(1).as_int() {
                        0 => old,
                        1 => {
                            cell.set(a);
                            0
                        }
                        2 => {
                            cell.set(old.wrapping_add(a));
                            old
                        }
                        _ => {
                            if old == a {
                                cell.set(b);
                                1
                            } else {
                                0
                            }
                        }
                    };
                    Ok(Value::int(i128::from(result)))
                }
                _ => panic("internal error: not an atomic integer"),
            },
            "shared_new" => Ok(handle(Handle::Shared(arg(0), Cell::new(1)))),
            "shared_clone" => match arg(0) {
                Value::Handle(h) => {
                    if let Handle::Shared(_, count) = &*h {
                        count.set(count.get() + 1);
                    }
                    Ok(Value::Handle(h))
                }
                _ => panic("internal error: not a shared value"),
            },
            "shared_get" | "shared_count" => match get(0).as_deref() {
                Some(Handle::Shared(value, count)) => Ok(if name == "shared_get" {
                    value.clone()
                } else {
                    Value::int(i128::try_from(count.get()).unwrap_or(0))
                }),
                _ => panic("internal error: not a shared value"),
            },
            other => panic(&format!("internal error: unknown intrinsic `{other}`")),
        }
    }
}
