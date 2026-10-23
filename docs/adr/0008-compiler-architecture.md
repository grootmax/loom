# ADR 0008: Compiler architecture

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [docs/internals/README.md](../internals/README.md), [ADR 0009](0009-backend-strategy.md)

## Context

The compiler must produce excellent diagnostics, power an IDE (incremental, error-tolerant),
be embeddable, and be maintainable by contributors for decades. rustc and rust-analyzer
have already learned most of these lessons at scale.

## Decision

1. **Pipeline (mirrors rustc):**
   `source → lexer → parser → lossless CST → AST → name resolution → HIR →
   type inference/checking → MIR (CFG) → borrow check, move/drop elaboration, Send analysis →
   MIR optimizations → backend`.
   Each stage is a separate crate under `compiler/` (see the internals guide's crate map).
2. **Hand-written recursive-descent parser** with Pratt parsing for expressions and error
   recovery. It never stops at the first error, and it always produces a tree containing
   error nodes.
3. **Lossless CST** in `loom_syntax`: green/red trees (immutable, shared, position-independent
   green nodes; parent-pointer red cursors), keeping every byte including trivia. The parser,
   formatter, LSP and refactorings share it. `parse → print` is the identity on every input,
   including invalid input. **We implement the tree ourselves** (~1–2k lines, modeled on
   rust-analyzer's `rowan`) rather than depending on `rowan`, because it is a small,
   critical, long-lived core.
4. **AST** is a set of typed, zero-cost views over the CST (rust-analyzer style), not a separate copy.
5. **Query-based, incremental from day one.** `loom_driver` hosts a demand-driven query
   database: functions of (input → output) that are memoized and invalidated by revision.
   **We use the `salsa` crate** (used by rust-analyzer and ty, with multiple maintainers),
   introduced in Phase 1 with an entry in `docs/dependencies.md`. Exit plan: salsa is
   confined to `loom_driver` behind our own query traits, so replacing it means touching one crate.
6. **Diagnostics are a product feature:** each has a stable code (`E0001`), a severity, a
   primary span with a label, secondary labeled spans, notes, and suggested fixes
   (machine-applicable where possible). A long explanation lives in
   `compiler/loom_diagnostics/explanations/E0001.md` and is shown by `loom explain`. The
   renderer is our own. Every diagnostic is snapshot-tested.
7. **Reference interpreter** (`loom_interp`) executes HIR in Phase 2 and MIR from Phase 3.
   It is the executable oracle that native backends are differentially tested against, and
   it powers `loom repl`.
8. **No global mutable state.** Everything goes through the database or explicit contexts,
   so the compiler is a library (embeddable by `loom-embed` and the LSP).
9. **Interning and arenas** for types and symbols, scoped to the database, not global.

## What Rust does

rustc pioneered AST → HIR → MIR, borrow checking on MIR, and query-based incremental
compilation, but retrofitted the queries onto an older design. rust-analyzer separately
built a lossless CST and salsa, and the two still don't share a front end. Loom starts
with one front end that serves both the compiler and the IDE, which is the lesson both
projects document.

## Alternatives considered

- **Parser generator (LALR, PEG):** rejected. Poor error recovery and poor messages.
- **Separate parsers for the compiler and the IDE:** rejected. Duplicated work and drift (the rustc/RA split).
- **Depending on `rowan`:** reasonable, but the tree is core infrastructure we want to own.
  Its design is our reference.
- **Building our own query engine:** rejected for now. Incrementality is subtle, salsa is
  proven, and the exit plan bounds the risk.

## Consequences

- Phase 1 must deliver the CST, recovery and diagnostics machinery before any semantics.
- IR dumps (`loom check --dump=hir|mir`) are snapshot-tested; their formats are unstable.
- Contributors from rustc/RA will recognize the architecture, and the internals guide can
  link to the rustc-dev-guide for background.
