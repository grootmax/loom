# Internals guide

How the Loom compiler, runtime and tools work, for contributors. Assume the reader has
zero context.

## Compiler pipeline (ADR 0008)

```
source ─► loom_lexer ─► loom_parser ─► loom_syntax (lossless CST) ─► loom_ast
       ─► loom_resolve ─► loom_hir ─► loom_check (+ loom_types)
       ─► loom_mir (borrow check, moves, Send) ─► loom_opt
       ─► loom_interp | loom_codegen_cranelift | (later) LLVM, wasm
```

`loom_driver` owns the query database and orchestrates the pipeline; `loom_cli` is the
`loom` binary. `loom_span` and `loom_diagnostics` are shared by every stage.

## Crate map

| Crate | Path | Phase | Role |
|---|---|---|---|
| loom_span | compiler/loom_span | 1 | Source files, spans, line maps |
| loom_diagnostics | compiler/loom_diagnostics | 1 | Error codes, labeled spans, rendering |
| loom_lexer | compiler/loom_lexer | 1 | Tokens with trivia |
| loom_syntax | compiler/loom_syntax | 1 | Lossless CST |
| loom_parser | compiler/loom_parser | 1 | Recursive descent with recovery |
| loom_ast | compiler/loom_ast | 1 | Typed AST views |
| loom_resolve | compiler/loom_resolve | 2 | Names, modules, imports |
| loom_hir | compiler/loom_hir | 2 | Desugared IR |
| loom_types | compiler/loom_types | 2 | Type representation and interning |
| loom_check | compiler/loom_check | 2 | Inference and checking |
| loom_interp | compiler/loom_interp | 2 | Reference interpreter |
| loom_mir | compiler/loom_mir | 3 | CFG IR, borrow checker |
| loom_opt | compiler/loom_opt | 3 | MIR optimizations |
| loom_codegen_cranelift | compiler/loom_codegen_cranelift | 3 | Native debug backend |
| loom_driver | compiler/loom_driver | 1–3 | Queries and orchestration |
| loom_cli | compiler/loom_cli | 0+ | The `loom` binary |
| loom_runtime | runtime | 3–4 | Scheduler, memory, channels, I/O |
| loom_fmt, loom_lint, loom_lsp, loom_doc, loom_pkg | tools/* | 1–6 | Tooling |

Each subsystem gets its own page here as it is implemented:

- [The front end: lexer, CST, parser, AST, formatter](front-end.md)
