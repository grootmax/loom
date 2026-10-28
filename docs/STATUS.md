# Status

**Current phase: 2 — Semantics** (started 2026-10-28 on the owner's go-ahead). Phase 1's
exit criteria are met locally except for the 24 CPU-hour fuzz run (in progress) and the
merge and tag, which wait on push access. This file is the honest answer to "what
works?" and must be updated whenever that changes.

_Last updated: 2026-10-28_

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
- `loom check [--dump=cst|--dump=hir] <paths>` reports lexical, syntax, name and type
  errors.
- `cargo xtask conformance` runs `spec/conformance/` (accept cases through `loom run`);
  `cargo xtask examples` runs every example whose phase has been reached.
- VS Code syntax highlighting for `.loom` files (`tools/editors/vscode`).

## In progress

- Phase 1 closure: a 24 CPU-hour run of a mutation fuzzer over lexer, parser and
  formatter (2 formatter bugs found and fixed so far); merging Jules's Phase 1 PRs.
- Phase 2: conformance coverage of every Phase 2 spec section (cases for §3–§10 exist;
  more are being written), diagnostic snapshot tests, the diagnostic renderer.

## Known limitations

- Ownership is not checked yet (Phase 3): programs that move a value twice or keep a
  borrow too long are accepted and run. `ref T` is checked as `T`. `Drop` does not run.
- `select`, `unsafe` and `extern "C"` report E0398 (not implemented in this phase).
- Programs are single files with inline modules; packages arrive in Phase 5.
- Map and set keys use structural hashing; a user `Eq` on a key type is honored for
  equality, but hashing is always structural.
- The formatter does not re-wrap long lines. Identifiers are not yet NFC-normalized.

## Written down (design, not implementation)

- ADRs 0001–0019 (`docs/adr/`), including the ownership model (0005), concurrency (0006),
  the front-end design (0018) and semantic analysis (0019).
- Spec: §2–§7, §9, §10, §12.1–§12.2, §12.4 and §14 are normative; §8, §11, §12.3 and §13
  are outlines.

## Open owner decisions

See the "Owner decisions" section in `CLAUDE.md` and ⚑ markers in ADRs 0005, 0009, 0014.
