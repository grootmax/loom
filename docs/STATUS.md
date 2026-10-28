# Status

**Current phase: 1 — Front end** (started 2026-10-28; Phase 0 tagged `v0.0.0-phase0`).
This file is the honest answer to "what works?" and must be updated whenever that changes.

_Last updated: 2026-10-28_

## Works

- Cargo workspace; `cargo xtask ci` passes (fmt, clippy pedantic, tests, docs, cargo-deny).
- `loom --version` and `loom help`. Every other command reports which phase implements it.
- CI on every Tier 1 host: Linux x86_64/aarch64 (glibc), Linux x86_64 musl, macOS
  aarch64/x86_64, Windows x86_64. `main` is protected: all CI jobs required, squash-only.
- `cargo xtask spec-check`: every grammar rule cites an existing spec section (run in CI).
- `loom_span`: byte spans, source map, 1-based line/column mapping (columns in scalar values).
- `loom_diagnostics`: diagnostic data model and the error-code registry, kept identical to
  spec Appendix C by a test.
- **Lexer** (`loom_lexer`): all of spec §2, lossless, with every Appendix C.1 diagnostic.
- **Parser** (`loom_parser`, `loom_syntax`): event-based recursive descent producing a
  lossless green/red CST with error recovery (E0100–E0113). All 28 examples parse cleanly.
- **AST** (`loom_ast`): typed views over the CST.
- **Formatter** (`loom fmt [--check]`): canonical style per `docs/reference/style.md`;
  idempotent and token-preserving on every example.
- `loom check [--dump=cst] <paths>` reports lexical and syntax errors (one line each;
  the snippet renderer is in progress). `loom explain <code>` prints the code's title.
- `cargo xtask conformance` runs `spec/conformance/` (front-end cases in Phase 1) and is
  part of `cargo xtask ci`.
- VS Code syntax highlighting for `.loom` files (`tools/editors/vscode`).

## In progress (Phase 1)

See `tasks/LEDGER.md` for delegated work: conformance cases, snapshot crate, Unicode
tables, error explanations, diagnostic renderer, `loom explain`, parser snapshot tests,
fuzzer, literal values. Remaining exit criteria: 24 CPU-hours of clean fuzzing and
snapshot coverage of every diagnostic.

## Written down (design, not implementation)

- ADRs 0001–0018 (`docs/adr/`), including the ownership model (0005), concurrency (0006)
  and the front-end design (0018).
- Spec: lexical structure complete (§2), diagnostic codes (Appendix C); other chapters
  outlined (`spec/loom-spec.md`).
- Grammar draft covering items, statements, expressions, patterns, types (`spec/grammar.ebnf`).
- 28 example programs with expected output (`examples/`).

## Does not work yet

- Name resolution, type checker, interpreter (Phase 2); code generation, runtime, std,
  packages, language server (later phases). The formatter does not re-wrap long lines yet.
  Identifiers are not yet NFC-normalized (spec §2.5 rule 2).

## Open owner decisions

See the "Owner decisions" section in `CLAUDE.md` and ⚑ markers in ADRs 0005, 0009, 0014.
