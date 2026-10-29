# Task 0022: Tests for `loom_runtime` and a Miri CI job

## Context
`runtime/` (crate `loom_runtime`) is the C-ABI runtime of native Loom programs
(`docs/adr/0021-native-backend-and-runtime.md` §4): `string.rs`, `array.rs`, `map.rs`,
`num.rs` and `lib.rs`. Generic collections receive a `TypeInfo` descriptor (size,
alignment, glue functions). It contains `unsafe` code, which ADR 0021 requires to be
tested under Miri. Read AGENTS.md and every file in `runtime/src/` first.

## Goal
Every exported function is exercised by unit tests, and CI runs those tests under Miri.

## Files you may change
- new files under `runtime/tests/` (integration tests calling the `extern "C"` functions
  from Rust)
- `.github/workflows/ci.yml`: add one job `miri` (Linux, nightly toolchain with the
  `miri` component, `cargo +nightly miri test -p loom_runtime`)

## Files you must NOT change
- `runtime/src/**` (list any bug you find under "Suspected runtime bugs" in the PR
  description with a failing test marked `#[ignore = "runtime bug: …"]`), everything else

## Requirements
1. Test-side descriptors: build `TypeInfo` values in the tests for `i64` (no glue),
   `String` handles (glue calling `loom_string_clone`/`loom_string_drop`/`loom_string_eq`/
   `loom_string_hash`/`loom_string_cmp`) and a small struct.
2. Cover: string construction, push, conversions, split, find, substring, repeat, chars
   round trip, eq/cmp/hash; arrays push/pop/get/insert/remove/slice/reverse/sort_by (with a
   test-side `extern "C"` comparison function)/clone/eq/cmp/debug/drop; maps insert
   (new and existing key), get, index, remove, entry, update, clone, eq, drop; number
   formatting (spec §7.12 rule 2 examples: `7.0`, `0.25`, `1e16`, `1.5e-7`, `NaN`, `inf`)
   and parsing (bounds of `I8`, `U8`, `I64`).
3. No leaks: every created object is dropped; Miri's leak check must pass.
4. Panicking paths (`loom_panic*`, out-of-bounds) exit the process; do not call them in
   tests.

## Acceptance
- `cargo test -p loom_runtime` and `cargo +nightly miri test -p loom_runtime` pass.
- `cargo xtask ci` passes; the new CI job is green.
- `git diff --stat origin/main` shows only the allowed paths.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `test(runtime): exercise the C API and run it under Miri`.
