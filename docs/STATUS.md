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
- VS Code syntax highlighting for `.loom` files (`tools/editors/vscode`).

## In progress (Phase 1)

See `tasks/LEDGER.md` for delegated work. Lexer, CST, parser, AST, renderer, `loom fmt`,
conformance runner and fuzzer are being built; design in ADR 0018.

## Written down (design, not implementation)

- ADRs 0001–0018 (`docs/adr/`), including the ownership model (0005), concurrency (0006)
  and the front-end design (0018).
- Spec: lexical structure complete (§2), diagnostic codes (Appendix C); other chapters
  outlined (`spec/loom-spec.md`).
- Grammar draft covering items, statements, expressions, patterns, types (`spec/grammar.ebnf`).
- 28 example programs with expected output (`examples/`).

## Does not work yet

- Lexer, parser, formatter (Phase 1, in progress); type checker, interpreter, code
  generation, runtime, std, packages, language server (later phases).

## Open owner decisions

See the "Owner decisions" section in `CLAUDE.md` and ⚑ markers in ADRs 0005, 0009, 0014.
