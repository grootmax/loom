# Status

**Current phase: 0 — Foundations.** Nothing runs yet. This file is the honest answer to
"what works?" and must be updated whenever that changes.

_Last updated: 2026-10-23_

## Works

- Cargo workspace with stub crates for every pipeline stage; `cargo xtask ci` passes
  (fmt, clippy pedantic, tests, docs, cargo-deny).
- `loom --version` and `loom help`. Every other command reports which phase implements it.
- CI on Ubuntu, macOS and Windows.
- VS Code syntax highlighting for `.loom` files (`tools/editors/vscode`).

## Written down (design, not implementation)

- ADRs 0001–0017 (`docs/adr/`), including the ownership model (0005) and concurrency (0006).
- Spec: lexical structure complete (§2); other chapters outlined (`spec/loom-spec.md`).
- Grammar draft covering items, statements, expressions, patterns, types (`spec/grammar.ebnf`).
- 28 example programs with expected output (`examples/`).

## Does not work yet

- Lexer, parser, type checker, interpreter, code generation, runtime, std, packages,
  language server, formatter: all stubs.

## Open owner decisions

See the "Owner decisions" section in `CLAUDE.md` and ⚑ markers in ADRs 0005, 0009, 0014.
