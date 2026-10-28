# Task 0020: Explanations for name, type, pattern and ownership errors

## Context
`loom explain EXXXX` prints `compiler/loom_diagnostics/explanations/EXXXX.md` (format in
that directory's `README.md`). Explanations exist for E0001–E0113. The codes
E0200–E0213, E0282, E0300–E0330, E0398, E0400–E0405 and E0500–E0504 (spec Appendix
C.4–C.6) have none. `compiler/loom_cli/tests/explanations.rs` checks examples with the
parser only. Read AGENTS.md, spec Appendix C and the spec section each code cites.

## Goal
Every code in `loom_diagnostics::codes::ALL` has an explanation whose example really
produces that code and whose fix really compiles without errors.

## Files you may change
- new files `compiler/loom_diagnostics/explanations/E02*.md`, `E03*.md`, `E04*.md`,
  `E05*.md`
- `compiler/loom_cli/tests/explanations.rs`: extend the checks to analyze examples with
  `loom_driver::Session` (`add_bytes`, then `analyze`; read `Analysis::diagnostics`) so
  name, type and ownership codes are verified; keep the existing checks
- `compiler/loom_cli/Cargo.toml`: only to add `loom_driver` as a dev-dependency if needed
- `Cargo.lock`: only the resulting change

## Files you must NOT change
- existing explanations, `compiler/` source files, `spec/`, everything else

## Requirements
1. One file per code listed above (except E0398 may say "not implemented yet in this
   phase" with no example). Follow the README's format exactly.
2. The example is the smallest program producing the code; the test asserts the code is
   among its errors. The fix is the same program corrected; the test asserts it has no
   errors at all. Both must be complete programs with `def main`.
3. Explain the *why* in plain words, in the voice of the existing explanations; never
   mention lifetimes. Link the spec section (`See spec §8.3.`).
4. A test fails if any code in `codes::ALL` lacks an explanation file.

## Acceptance
- `cargo test -p loom_cli` passes; `cargo xtask ci` passes.
- `cargo run -p loom_cli -- explain E0502` prints the new explanation.
- `git diff --stat origin/main` shows only the allowed paths.

## Notes
- Follow AGENTS.md. Never weaken or delete existing tests.
- Conventional Commit title: `docs(diagnostics): explanations for name, type, pattern and ownership errors`.
