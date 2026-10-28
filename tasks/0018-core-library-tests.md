# Task 0018: Tests for the core library (`std/core`)

## Context
The core library is written in Loom (`std/core/*.loom`, ADR 0019 §4) over a small list of
intrinsics (`docs/internals/intrinsics.md`). Its public API is decided; its behavior
needs tests. Loom tests are functions marked `@[test]` run by `loom test <file>`
(example 27 shows the output format). Read AGENTS.md and every file in `std/core/`.

## Goal
Every public function and method of the core library is exercised by `@[test]` functions,
including edge cases (empty collections, out-of-range indices that panic, overflow).

## Files you may change
- new directory `std/core/tests/` with one `<module>_test.loom` per core file
  (`array_test.loom`, `string_test.loom`, …)
- `xtask/src/main.rs` and a new `xtask/src/core_tests.rs`: a `cargo xtask core-tests`
  command that runs `loom test` on every file in `std/core/tests/`, and adds it to `ci`
  after `examples`

## Files you must NOT change
- `std/core/*.loom` (if a function misbehaves, list it under "Suspected bugs" in the PR
  description and leave its test out), `compiler/`, `spec/`, everything else

## Requirements
1. Each test function is small and named after what it checks
   (`array_pop_on_empty_returns_none`).
2. Use `assert` and `assert_eq` only. A test that is expected to panic cannot be
   expressed yet; skip those cases and list them in the PR description.
3. `cargo xtask core-tests` prints one summary line and fails if any test fails.
4. Target: at least 150 tests across all core files.

## Acceptance
- `cargo xtask core-tests` passes; `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- Changing the core library or the interpreter.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `test(core): exercise the core library with loom test`.
