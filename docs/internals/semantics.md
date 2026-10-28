# Semantic analysis and the interpreter (Phase 2)

How a parsed program becomes a checked, runnable one. The decisions behind this page are
in [ADR 0019](../adr/0019-semantic-analysis-and-interpreter.md); the language rules are in
spec §3–§7, §9, §10, §12 and §14.

## Pipeline

```
Session::analyze(file)
  parse core files + the program
  loom_hir::lower_file    → Hir { defs (item tree), bodies (arenas) }
  loom_resolve::resolve   → Resolution { def_map, per-body side tables, diagnostics }
  loom_check::check       → Checked { interner, items, per-body TypedBody, diagnostics }
loom_interp::Interp::run_main / run_tests
```

Everything is rebuilt per program. The query-shaped API (`Session::analyze`, `Checked`)
leaves room for incremental memoization when the language server needs it (Phase 6).

## HIR (`loom_hir`)

- `DefId` indexes `Hir::defs`: modules, structs, enums, variants, traits, impls,
  functions, constants, type aliases and associated types. Items are allocated parent
  first, so a method's generics parent always has a smaller id.
- `Body` holds arenas of `Expr`, `Pat` and `Binding`, indexed by `ExprId`, `PatId` and
  `BindingId`, plus the source range of each. A function's body also holds its
  parameters' default values.
- Lowering is syntactic and total. Desugarings: `@x` → `self.x`; command calls and
  trailing blocks → ordinary calls; `elsif` → nested `if`; interpolated strings →
  `Expr::Interpolate`. `Name[Args]` keeps both readings (index and type arguments) until
  resolution says which applies.
- `loom check --dump=hir` prints the bodies of the program (not of `core`).

## Name resolution (`loom_resolve`)

- `DefMap` gives each module two namespaces (types and values); imports are resolved to
  a fixed point. The prelude is the `pub use` list of `std/core/prelude.loom`.
- Each body gets a `BodyResolution`: what every name refers to, which `x = e` statements
  are assignments, which capitalized names were not found (unqualified variants, left to
  the checker), and the aliasing of or-pattern bindings.
- Type paths are resolved on demand by the checker with `DefMap::resolve_type_path`.

## Types and checking (`loom_types`, `loom_check`)

- `Ty` is an interned `u32`. Inference variables are union-find nodes in `InferTable`;
  integer and float variables only unify with integer and float types and default to
  `Int` and `Float` at the end of a body.
- `items::collect` lowers every signature: generics and predicates, fields, variants,
  function signatures, traits, impls (including `@[derive]` impls).
- `traits::Solver` selects implementations (user impls, blanket impls with bounds,
  derived impls, the built-in impls of primitives and tuples, `dyn`, and bounds in
  scope) and normalizes associated types. The interpreter uses the same solver with
  concrete types.
- `infer` checks one body bidirectionally. Arguments are checked in two passes so
  closures see solved generic parameters. Method lookup tries inherent methods, then
  traits in scope or in bounds. Trait method calls are recorded as
  `CallTarget::Trait` and dispatched at run time on the concrete type.
- `exhaustive` implements Maranget's usefulness algorithm for `case` exhaustiveness,
  unreachable arms and irrefutable patterns.

## The interpreter (`loom_interp`)

- Values: scalars inline, everything else behind `Rc` with copy-on-write.
- A `Place` is a cell plus a projection path. Each binding has a place; a `mut`
  argument or receiver passes the caller's place, so writes reach the caller.
- Closures copy the frame's slot table (sharing the cells), so they see later writes
  to captured bindings but not later re-declarations.
- Generic code runs with the frame's concrete substitution; trait methods are resolved
  for the concrete type with `Solver::select`.
- Panics are `Flow::Panic` values carrying a span; `main` reports them as
  `panicked at file:line:col: message` with exit status 101.

## Testing

- `spec/conformance/` accept cases run through `loom run`; reject cases check codes and
  positions (`cargo xtask conformance`).
- `cargo xtask examples` runs every example of a reached phase.
