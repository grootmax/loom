//! The reference interpreter (ADR 0008 §7, ADR 0019 §8).
//!
//! It evaluates HIR using the checker's [`TypedBody`] for every type and every call
//! resolution; it never re-derives either. Generic code runs with the frame's concrete
//! substitution, so trait methods dispatch on concrete types.

mod eval;
mod intrinsics;
mod ops;
mod value;

pub use value::{Place, Value};

use loom_check::{Checked, Env, Items, Solver, TypedBody};
use loom_hir::{BodyId, DefId, Hir, Name};
use loom_span::{SourceMap, Span};
use loom_types::{InferTable, Interner, Subst, Ty};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

/// The maximum call depth before the program panics with "stack overflow".
pub const MAX_DEPTH: usize = 10_000;

/// Why evaluation stopped early.
#[derive(Debug)]
pub enum Flow {
    /// `return`.
    Return(Value),
    /// `break`.
    Break(Value),
    /// `next`.
    Next,
    /// A panic with its message and location.
    Panic(String, Option<Span>),
}

/// The result of evaluating something.
pub type Eval<T> = Result<T, Flow>;

/// How a program ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// `main` returned normally.
    Finished,
    /// `main` returned `Err`; the message to print.
    Error(String),
    /// The program panicked.
    Panicked(String),
}

impl Outcome {
    /// The process exit status (spec §14.2).
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Finished => 0,
            Self::Error(_) => 1,
            Self::Panicked(_) => 101,
        }
    }
}

/// The interpreter for one checked program.
pub struct Interp<'a> {
    hir: &'a Hir,
    checked: &'a Checked,
    sources: &'a SourceMap,
    out: RefCell<Box<dyn Write + 'a>>,
    depth: Cell<usize>,
    consts: RefCell<HashMap<DefId, Value>>,
}

impl<'a> Interp<'a> {
    /// An interpreter writing program output to `out`.
    pub fn new(
        hir: &'a Hir,
        checked: &'a Checked,
        sources: &'a SourceMap,
        out: Box<dyn Write + 'a>,
    ) -> Self {
        Self {
            hir,
            checked,
            sources,
            out: RefCell::new(out),
            depth: Cell::new(0),
            consts: RefCell::new(HashMap::new()),
        }
    }

    fn interner(&self) -> &'a Interner {
        &self.checked.interner
    }

    fn items(&self) -> &'a Items {
        &self.checked.items
    }

    fn typed(&self, body: BodyId) -> &'a TypedBody {
        &self.checked.bodies[&body]
    }

    /// Writes program output.
    fn write_out(&self, text: &str) {
        let _ = self.out.borrow_mut().write_all(text.as_bytes());
    }

    /// Flushes program output.
    pub fn flush(&self) {
        let _ = self.out.borrow_mut().flush();
    }

    /// Normalizes associated types in a concrete type.
    fn normalize(&self, ty: Ty) -> Ty {
        let mut table = InferTable::new();
        Solver {
            hir: self.hir,
            items: self.items(),
            interner: self.interner(),
            table: &mut table,
        }
        .normalize(ty, &Env::default())
    }

    /// Makes `ty` concrete under `subst` (the frame's generic arguments).
    fn concrete(&self, ty: Ty, subst: &Subst) -> Ty {
        let ty = self.interner().subst(ty, subst);
        self.normalize(ty)
    }

    fn panic_message(&self, message: &str, span: Option<Span>) -> String {
        match span {
            Some(span) if span.file.raw() != u32::MAX => {
                let file = self.sources.file(span.file);
                let position = file.line_column(span.range.start());
                format!(
                    "panicked at {}:{}:{}: {message}",
                    file.name(),
                    position.line,
                    position.column
                )
            }
            _ => format!("panicked: {message}"),
        }
    }

    /// Runs `main` (spec §14).
    pub fn run_main(&self, main: DefId) -> Outcome {
        let result = self.call_fn(main, &Subst::new(), None, Vec::new(), None);
        self.flush();
        match result {
            Ok(value) => match value {
                Value::Enum(e) if self.hir.def(e.variant).name.as_str() == "Err" => {
                    let error = e.fields.first().cloned().unwrap_or(Value::Unit);
                    let message = self
                        .error_message(&error)
                        .unwrap_or_else(|_| "error".to_owned());
                    Outcome::Error(format!("Error: {message}"))
                }
                _ => Outcome::Finished,
            },
            Err(Flow::Panic(message, span)) => {
                Outcome::Panicked(self.panic_message(&message, span))
            }
            Err(_) => Outcome::Finished,
        }
    }

    fn error_message(&self, error: &Value) -> Eval<String> {
        let ty = self.value_ty(error);
        let value = self.call_trait_method(
            "Error",
            "message",
            ty,
            eval::ArgVal::Value(error.clone()),
            Vec::new(),
            None,
        )?;
        Ok(value.as_str().to_owned())
    }

    /// Runs every `@[test]` function of the root module and its inline modules, writing
    /// the report of example 27. Returns whether all passed.
    pub fn run_tests(&self, root: DefId) -> bool {
        let tests: Vec<DefId> = self
            .hir
            .iter_defs()
            .filter(|(_, d)| {
                d.package == loom_hir::Package::Main
                    && d.attr("test").is_some()
                    && self.is_in(d.module, root)
            })
            .map(|(id, _)| id)
            .collect();
        self.write_out(&format!(
            "running {} test{}\n",
            tests.len(),
            if tests.len() == 1 { "" } else { "s" }
        ));
        let mut failures = Vec::new();
        for &test in &tests {
            let name = self.hir.path_string(test);
            match self.call_fn(test, &Subst::new(), None, Vec::new(), None) {
                Err(Flow::Panic(message, span)) => {
                    self.write_out(&format!("test {name} ... FAILED\n"));
                    failures.push((name, self.panic_message(&message, span)));
                }
                _ => self.write_out(&format!("test {name} ... ok\n")),
            }
        }
        if !failures.is_empty() {
            self.write_out("\nfailures:\n");
            for (name, message) in &failures {
                self.write_out(&format!("  {name}: {message}\n"));
            }
        }
        let passed = tests.len() - failures.len();
        self.write_out(&format!(
            "\ntest result: {}. {passed} passed; {} failed\n",
            if failures.is_empty() { "ok" } else { "FAILED" },
            failures.len()
        ));
        self.flush();
        failures.is_empty()
    }

    fn is_in(&self, module: DefId, root: DefId) -> bool {
        let mut current = Some(module);
        while let Some(m) = current {
            if m == root {
                return true;
            }
            current = self.hir.def(m).parent;
        }
        false
    }

    fn method_named(&self, trait_lang: &str, name: &str) -> Option<DefId> {
        let trait_ = self.items().lang.get(trait_lang)?;
        self.items()
            .traits
            .get(&trait_)?
            .methods
            .get(&Name::new(name))
            .copied()
    }
}

/// Shared helper for owned subst values.
fn rc_subst(subst: Subst) -> Rc<Subst> {
    Rc::new(subst)
}
