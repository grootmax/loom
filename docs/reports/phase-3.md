# Phase 3 report: native code

- **Tag:** `v0.3.0-phase3` (commit `88812d8`, PR #33)
- **Dates:** 2026-10-29
- **Design:** [ADR 0020](../adr/0020-mir-and-ownership-checking.md) (MIR and ownership
  checking), [ADR 0021](../adr/0021-native-backend-and-runtime.md) (native backend and
  runtime); spec §8 became normative.

## Exit criteria

| Criterion | Result |
|---|---|
| Native binaries on all Tier 1 targets | Met. On every Tier 1 CI host, `cargo xtask differential --aot` builds each program with `loom build` and runs the executable: Linux x86-64 and AArch64, macOS AArch64 and x86-64, and Windows x86-64 (MSVC `link.exe`). Musl is tested with `cargo test` only. |
| Differential tests match the interpreter | Met. 350 programs (every example and conformance accept case) give identical stdout and exit status with the JIT and as executables. This is part of `cargo xtask ci`. |
| Borrow-checker conformance (accept and reject cases) passes | Met. 546 conformance cases pass. They include the §8 ownership cases: 34 reject cases with E0500–E0504 and their accept cases. |
| No leaks or use-after-free under Miri/sanitizers | Met. The runtime's tests run under Miri in CI. Every native executable runs under Valgrind in CI (`--leak-check=full`, errors fail the job). Every native run checks the runtime's live-object count (`LOOM_LEAK_CHECK=1`). |

## What was built

- **Part 1 (#23):**
  - `ref T` as a real type.
  - MIR for every function and closure.
  - The ownership and borrow checker, with non-lexical lifetimes, two-phase borrows and borrows returned by elision.
- **Part 2 (#33):**
  - `loom_runtime`, a C-ABI runtime: strings, arrays, maps, formatting, panics, and type descriptors.
  - `loom_codegen_cranelift`, which monomorphizes MIR and generates per-type glue, vtables, closure environments and drop flags with per-field paths.
  - `loom run` (JIT, with interpreter fallback), `loom build`, and `cargo xtask differential`.
- **Performance:** native code runs about 100× faster than the interpreter on a recursion plus sieve benchmark (0.02 s against 2.13 s, release build).

## Bugs found by testing

The differential test, Miri and the leak checker found these bugs. Each fix has a test.

- **MIR builder:**
  - A borrow returned by a call was copied in a borrow position.
  - Temporaries were never dropped in expression bodies or on early exits.
  - Default argument values were never dropped.
- **Code generator:** an assignment after a partial move freed the moved value (use after free). Per-field flags fixed it; drops now come only from MIR.
- **Interpreter:** escaping closures shared their captures instead of copying them (spec §8.4).
- **Runtime:** aliasing violations in the debug, eq and cmp functions (found by Miri tests from Jules).
- **Nightly fuzz workflow:** it was invalid YAML, caught by actionlint (#34).

## Delegation

Jules delivered:

- §8 conformance (#27).
- Error explanations (#26).
- Rich diagnostics (#25).
- Runtime tests under Miri (the commit was taken from #30).
- The nightly fuzz workflow (#29).
- Items and statements conformance (#31).

Lessons are in ADR 0015, and AGENTS.md has a new learned rule 5.

## Known limitations carried forward

- **Native code:**
  - 128-bit arithmetic, borrowed `dyn` and closures with `own` parameters fall back to the interpreter.
  - `loom test` runs in the interpreter.
- **Language and runtime:**
  - No user `Drop` trait yet.
  - Panics do not run drops (no unwinding).
