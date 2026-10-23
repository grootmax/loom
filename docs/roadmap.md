# Roadmap

Work proceeds strictly phase by phase. Each phase ends when all exit criteria are met,
[STATUS.md](STATUS.md) is updated, a release is tagged, and the owner has received a
report and approved starting the next phase.

| Phase | Goal | Exit criteria |
|---|---|---|
| **0. Foundations** | Repository, governance, design, spec skeleton, CI, agent workflow | ADRs 0001–0017, spec skeleton and grammar, ≥25 examples, CI green on Linux/macOS/Windows, delegation workflow piloted |
| **1. Front end** | Lexer, parser, lossless CST, AST, diagnostics engine, `loom fmt` | Parses all examples; parser fuzzed 24 h clean; round-trip and formatter-idempotence properties pass; diagnostics snapshot-tested |
| **2. Semantics** | Name resolution, HIR, type inference and checking, reference interpreter | `loom run hello.loom` works via the interpreter; conformance suite covers expressions, functions, structs, enums, traits, generics, pattern matching, `Option`/`Result` |
| **3. Native code** | MIR, borrow checker, move/drop elaboration, Cranelift backend | Native binaries on all Tier 1 targets; differential tests match the interpreter; borrow-checker conformance (accept and reject cases) passes; no leaks or use-after-free under Miri/sanitizers |
| **4. Concurrency** | Scheduler, tasks, scopes, channels, `select`, async I/O reactor, `Send` checking | 1M concurrent tasks benchmark; deterministic-scheduler race tests; TCP echo benchmark published |
| **5. Std, packages, Rust interop** | `core`/`std`, `loom.toml`, lockfile, MVS, `loom add/publish`, `loom-bridge`, `loom-embed` | A real HTTP JSON API and a CLI tool using only std; ≥5 popular Rust crates usable via generated bindings; a Rust app calls Loom; reproducible builds verified |
| **6. Tooling** | LSP, `loom doc`, REPL, test/bench polish, DWARF/PDB, VS Code extension, tree-sitter grammar | Smooth editor experience in VS Code on all Tier 1 OSes |
| **7. Hardening → 1.0** | Spec freeze for the first edition, performance, LLVM release backend, security review, complete docs | Beta → RC → 1.0 with the compatibility promise published |
| **8. Web framework** | Separate repository | A real app deployed as a single binary/container |
| **9. Desktop framework** | Separate repository | Same app on Linux, macOS, Windows |
| **10. Mobile framework** | Separate repository; mobile targets promoted to Tier 1 | Sample app on iOS and Android |
| **11. Cloud ecosystem** | Observability, serverless adapters, provider SDK packages | Reference deployments on major clouds |

Frameworks (Phases 8–11) do not start before the Phase 7 beta: a framework built on a
moving language gets rewritten repeatedly.
