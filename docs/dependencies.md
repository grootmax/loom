# Dependencies

Every external crate the toolchain depends on is listed here with a one-line
justification. **Prefer zero.** Long-term survival depends on owning our critical code.

Rules:

1. A new dependency needs a row here, in the same PR, and reviewer approval.
2. No unmaintained or single-maintainer crate on a core path (compiler, runtime, std)
   without a written exit plan (vendor, fork, or replace).
3. Licenses must pass `cargo deny` (`deny.toml`).
4. Dev-only dependencies (tests, benches, fuzzing) are listed too, marked `dev`.

| Crate | Version | Used by | Kind | Why we need it | Exit plan |
|---|---|---|---|---|---|
| `cranelift-codegen` | =0.137.0 | `loom_codegen_cranelift` | normal | Machine-code generation for the native backend (ADR 0009, 0021): instruction selection, register allocation, encoding for x86-64 and AArch64 | LLVM backend in Phase 7 (ADR 0009); the interpreter stays the reference. Maintained by the Bytecode Alliance (Wasmtime), many maintainers |
| `cranelift-frontend` | =0.137.0 | `loom_codegen_cranelift` | normal | Builds Cranelift IR from MIR (blocks, switches, memory copies) | Same as `cranelift-codegen` |
| `cranelift-module` | =0.137.0 | `loom_codegen_cranelift` | normal | Functions, data and symbols shared by the JIT and object outputs | Same as `cranelift-codegen` |
| `cranelift-jit` | =0.137.0 | `loom_codegen_cranelift` | normal | In-process compilation for `loom run` (ADR 0021 §1) | Fall back to `loom build` plus running the executable |
| `cranelift-object` | =0.137.0 | `loom_codegen_cranelift` | normal | Object files for `loom build` | Same as `cranelift-codegen` |
| `target-lexicon` | 0.13 | `loom_codegen_cranelift` | normal | The host target triple, as Cranelift expects it | Tiny; replace with a table of Tier 1 triples |

The Cranelift crates pull in about 50 transitive crates (`regalloc2`, `gimli`, `object`,
`hashbrown`, `smallvec`, `log`, `region`, `libc`, platform crates and the Cranelift
sub-crates), all under licenses `deny.toml` allows. They are confined to the backend crate.
The runtime (`runtime/`) and the front end stay dependency-free.

<!-- Template row:
| `insta` | 1.x | compiler/* | dev | Snapshot tests for diagnostics and IR dumps | Replace with in-house snapshot helper (~300 LOC) |
-->
