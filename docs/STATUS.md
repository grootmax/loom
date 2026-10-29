# Status

**Current phase: 3 — Native code** (started 2026-10-29 on the owner's go-ahead). Phases 1
and 2 are complete and tagged `v0.1.0-phase1` and `v0.2.0-phase2`; their reports are in
[`docs/reports/`](reports/). This file is the honest answer to "what works?" and must be
updated whenever that changes.

_Last updated: 2026-10-29_

## Works

- Cargo workspace; `cargo xtask ci` passes (spec-check, fmt, clippy pedantic, tests, docs,
  conformance, examples, cargo-deny).
- CI on every Tier 1 host: Linux x86_64/aarch64 (glibc), Linux x86_64 musl, macOS
  aarch64/x86_64, Windows x86_64. `main` is protected: all CI jobs required, squash-only.
- **Front end (Phase 1):** lexer (all of spec §2), lossless CST, parser with error
  recovery, typed AST, literal values, diagnostics with stable codes (Appendix C), the
  canonical formatter (`loom fmt [--check]`), `loom explain <code>` (title only).
- **Semantics (Phase 2):**
  - HIR lowering, name resolution (modules, imports, visibility, scopes, binding vs.
    assignment), type inference and checking (generics with bounds, traits with
    associated types, blanket and derived impls, `dyn`, closures, `?`), exhaustiveness
    of `case`, and the Phase 2 diagnostics E0200–E0405.
  - The core library in Loom (`std/core/`): primitives, `String`, `Array`, `Map`, `Set`,
    `Option`, `Result`, `Range`, iterators, the core traits, and `puts`/`assert`.
  - The reference interpreter: `loom run <file>` and `loom test <file>`. All 19 examples
    marked `# phase: 2` produce their expected output byte for byte.
- **Ownership (Phase 3, part 1):** `ref T` is a real type with auto-deref and autoref;
  MIR is built for every function and closure (`loom check --dump=mir`); the borrow
  checker reports use after move (E0500/E0501), conflicting borrows (E0502), escaping
  borrows (E0503) and returned borrows whose source is ambiguous (E0504), with
  non-lexical lifetimes and two-phase borrows (ADR 0020, spec §8).
- `loom check [--dump=cst|hir|mir] <paths>` reports lexical, syntax, name, type and
  ownership errors.
- `cargo xtask conformance` runs `spec/conformance/` (accept cases through `loom run`);
  `cargo xtask examples` runs every example whose phase has been reached.
- VS Code syntax highlighting for `.loom` files (`tools/editors/vscode`).

## In progress

- Phase 3 exit: native executables checked on every Tier 1 host in CI (Windows links with
  MSVC's `link.exe`), Valgrind over native executables, the Phase 3 report.
- Conformance for §8 ownership and rich CLI diagnostics (Jules PRs #25, #27); the nightly
  fuzz workflow is fixed in #34.

## Known limitations

- Programs run only in the interpreter; native code arrives in Phase 3 part 2. `Drop`
  does not run yet.
- Explicit lifetimes are not part of the language (owner decision, 2026-10-29): a
  function may return a borrow only when its source is unambiguous (spec §8.5).
- `select`, `unsafe` and `extern "C"` report E0398 (not implemented in this phase).
- Programs are single files with inline modules; packages arrive in Phase 5.
- Map and set keys use structural hashing; a user `Eq` on a key type is honored for
  equality, but hashing is always structural.
- The formatter does not re-wrap long lines. Identifiers are not yet NFC-normalized.

## Written down (design, not implementation)

- ADRs 0001–0020 (`docs/adr/`), including the ownership model (0005), concurrency (0006),
  the front-end design (0018), semantic analysis (0019), and MIR and ownership checking
  (0020).
- Spec: §2–§10, §12.1–§12.2, §12.4 and §14 are normative; §11, §12.3 and §13 are
  outlines.

## Open owner decisions

See the "Owner decisions" section in `CLAUDE.md` and ⚑ markers in ADRs 0005, 0009, 0014.
