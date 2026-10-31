# Task 0024: Conformance cases for integer and float arithmetic (§7.5, §7.12)

## Context
Spec §7.5 defines the arithmetic operators: integer overflow, division by zero and
out-of-range shifts panic (exit status 101, §10.4), and `checked_*`, `wrapping_*` and
`saturating_*` methods (in `std/core/primitives.loom`) choose another behavior. Spec §7.12
rule 2 fixes how numbers print. `loom run` now runs programs natively (ADR 0021), and
`cargo xtask differential` compares native code with the interpreter on every
conformance accept case, so these cases test both. Read spec §7.5, §7.12, §10.4,
`spec/conformance/README.md` and AGENTS.md first.

## Goal
Every integer type and both float types have accept cases for their operators, overflow
behaviors and printing.

## Files you may change
- new files `spec/conformance/07-expressions/7_5_int_*` and `7_12_number_text_*`
  (`.loom`, `.stdout`, and `.exit` for cases that panic)

## Files you must NOT change
- everything else (in particular existing conformance files and the compiler)

## Requirements
1. For each of `I8`, `I16`, `I32`, `Int`, `U8`, `U16`, `U32`, `U64`: `+ - * / %` near the
   bounds (`T.max`, `T.min` from the core library), `checked_add/sub/mul/div` (both
   `Some` and `None`), `wrapping_add/sub/mul`, `saturating_add/sub/mul`, bit operators,
   `<<` and `>>` (including `>>` of negative values), comparison operators, and `abs`/`pow`
   where the core library has them.
2. One case per panic: overflow of `+`, `-`, `*` and unary `-` for a signed and an
   unsigned type, `/` and `%` by zero, `T.min / -1`, and a shift by the bit width. Each
   panicking case prints a line before the panic; its `.exit` file contains `101`.
3. Float printing (§7.12 rule 2): integral values (`7.0`), small and large magnitudes
   (`1e-7`, `1e16`, `1.5e300`), `0.1 + 0.2`, negative zero, `NaN` and infinities from
   `0.0 / 0.0` and `1.0 / 0.0`, and `F32` values.
4. Every case passes `cargo xtask conformance` and `cargo xtask differential`. The
   expected `.stdout` must follow the spec; if the implementation disagrees with the
   spec, keep the spec's answer, mark the case with a second line `# phase: 4`, and
   list it under "Suspected bugs" in the PR description.

## Acceptance
- `cargo xtask conformance` and `cargo xtask differential` pass.
- `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only new files under `spec/conformance/07-expressions/`.

## Notes
- Follow AGENTS.md (rule 4: run every file through `loom check` and `loom run`).
- Use only syntax from `spec/grammar.ebnf` and `examples/`.
