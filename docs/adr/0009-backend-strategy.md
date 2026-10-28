# ADR 0009: Backend strategy

- **Status:** Accepted; the Rust-emit question is deferred by the owner (2026-10-29)
- **Date:** 2026-10-23
- **Deciders:** lead architect; project owner for the Rust-emit question
- **Related:** [ADR 0008](0008-compiler-architecture.md), [ADR 0012](0012-platform-tiers.md), [ADR 0016](0016-rust-interop.md)

## Context

Loom needs fast debug builds (Go-like feedback), optimized release builds (Rust-like
speed), cross-compilation by flag, and later WebAssembly. A backend also determines how
much of the toolchain a user must install.

## Decision

1. **Order of backends:**
   1. **Reference interpreter** (Phase 2). The spec oracle; it also backs `loom run` until
      Phase 3.
   2. **Cranelift** (Phase 3). The default for `loom run`/`loom build`. It is fast to compile,
      written in Rust, supports x86_64 and aarch64 on all Tier 1 OSes, and supports unwinding.
   3. **LLVM** (Phase 7), for `loom build --release`. It has the best optimization and the
      widest target coverage, and is linked via the C API behind a cargo feature, so
      contributors can build without LLVM.
   4. **WebAssembly** (after LLVM). Via Cranelift's or LLVM's wasm target first, and a
      dedicated backend only if the component model requires one.
2. **All backends consume the same optimized MIR** and are differentially tested against the
   interpreter on every conformance and example program.
3. **Linking:** we ship or locate a linker per target (system linker by default; bundled
   `lld` for cross-compilation in Phase 7). Static linking of the runtime is always used.
4. **Rust-emit backend (generate Rust source, compile with rustc):** **not adopted as a
   primary backend.** It is recommended as an *optional, non-default* mode, for:
   - differential testing (a second, independent lowering);
   - bootstrapping a target that Cranelift does not support yet.

   It may be built only after Phase 3, if the owner approves.

## Rust-emit evaluation

| Criterion | Rust-emit | Own Cranelift/LLVM |
|---|---|---|
| Targets and optimizations | Everything rustc supports, for free | Cranelift: fewer targets; LLVM: equal |
| Rust crate interop | Trivial at the source level | Needs the C-ABI bridge (ADR 0016), which we need anyway for stable ABI |
| Second safety net | rustc's borrow checker re-checks our output | No |
| Compile speed | Slow: we would add rustc's time to ours, against the Go-like feedback goal | Cranelift is fast |
| User dependencies | Requires a Rust toolchain on every user machine | Self-contained `loom` binary |
| Debugging and errors | Debug info and panics map to generated `.rs`, then must be mapped back | Direct DWARF/PDB for `.loom` sources |
| Long-term independence | Ties the language to rustc's evolution and edition changes | Our MIR is the contract |

The deciding factors are compile speed (pillar 4) and self-sufficiency (longevity). The
benefits (free targets, a second checker) are real but achievable in other ways: LLVM for
targets, and a differential interpreter plus fuzzing for checking.

## What Rust does

rustc owns its MIR and uses LLVM, plus `rustc_codegen_cranelift` for fast debug builds
and `rustc_codegen_gcc` for extra targets. Loom copies this layering: own the IR and plug
in backends.

## Alternatives considered

- **LLVM only:** slow debug builds and a heavy build dependency for contributors.
- **Cranelift only:** weaker optimization; not enough for "fast as Rust".
- **C as a target language:** portable, but undefined-behavior traps, poor debug info, and
  a C compiler would be needed at user build time.
- **Rust-emit as the primary backend:** rejected as primary for the reasons above.

## Consequences

- The MIR must be documented as the stable internal contract between the front end and backends.
- The CI matrix must run differential tests per backend.
- ⚑ **Owner decision (2026-10-29): deferred.** Phase 3 builds only Cranelift. The optional
  Rust-emit mode is revisited with the owner after Phase 3, with data from differential
  testing.
