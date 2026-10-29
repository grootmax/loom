# Phase 2 report: semantics

- **Tag:** `v0.2.0-phase2` (commit `09ee4b0`, PR #22)
- **Dates:** 2026-10-28 to 2026-10-29
- **Design:** [ADR 0019](../adr/0019-semantic-analysis-and-interpreter.md); spec §3–§7,
  §9, §10, §12 and §14 became normative

## Exit criteria

| Criterion | Result |
|---|---|
| `loom run hello.loom` works via the interpreter | Met. All 19 examples marked `# phase: 2` produce their expected output byte for byte. |
| Conformance covers expressions, functions, structs, enums, traits, generics, pattern matching, `Option`/`Result` | Met: §7 expressions (#20); §5/§6 types, inference, traits and generics (#22); §9/§10/§14 patterns, error handling and execution (#21). At the tag, 334 cases pass and 2 are skipped. Items and statements (§3, §4, §12) also have cases, and a deeper set is in progress (task 0014). |

## What was built

- `loom_hir` (lowering), `loom_resolve` (two namespaces, modules, imports, visibility),
  `loom_types` (interned types), `loom_check`:
  - bidirectional inference;
  - a trait solver with blanket, derived and builtin impls and associated-type
    normalization;
  - Maranget exhaustiveness.
- The core library written in Loom (`std/core/`), with intrinsics listed in
  `docs/internals/intrinsics.md`, plus 160 core-library tests (`cargo xtask core-tests`,
  #19).
- `loom_interp`: the reference interpreter (copy-on-write values, places).
- `loom run`, `loom test`, and `loom check --dump=hir`.

## Bugs found by testing

- Function types counted the return type as a parameter.
- Integer and general type variables were unified in the wrong direction.
- Literal patterns lowered to "match anything".
- Explicit generic arguments were recorded on the wrong node.
- Argument errors cascaded.

Each fix has a regression case.

## Known limitations carried forward (addressed in Phase 3)

- Ownership was not checked; `ref T` was checked as `T`. This is fixed by Phase 3 part 1
  (#23).
- `Drop` does not run.
- `select`, `unsafe` and `extern "C"` report E0398.
