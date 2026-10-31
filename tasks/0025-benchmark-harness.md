# Task 0025: Benchmark harness (`cargo xtask bench`)

## Context
Loom now has a native backend (ADR 0021): `loom run` uses the JIT, `loom run --interp`
the reference interpreter, and `loom build -o <exe> <file>` writes an executable. Phase 4
needs throughput numbers (ADR 0006), and the roadmap asks for published benchmarks. The
harness must use only the standard library (xtask is dependency-free). Read
`xtask/src/main.rs`, `xtask/src/differential.rs` and AGENTS.md first.

## Goal
`cargo xtask bench` builds every program in `benches/` natively, runs it several times,
and prints a table of median wall-clock times for the native executable and the
interpreter.

## Files you may change
- new directory `benches/` with 5–8 `.loom` programs and a `README.md`
- new file `xtask/src/bench.rs`
- `xtask/src/main.rs`: only the `mod bench;` line, the `Some("bench")` match arm and the
  help line

## Files you must NOT change
- everything else

## Requirements
1. Programs (each prints a checksum so work can't be skipped, and runs 0.1–2 s natively in
   a release build): recursive `fib`, a prime sieve over an `Array[Bool]`, string
   building and splitting, `Map[String, Int]` word counting, sorting 100 000 integers with
   `sort_by`, struct-heavy code (an n-body step or similar), closures with `map`/`filter`.
2. `cargo xtask bench [--runs N] [--interp] [--filter NAME]`: builds `loom` in release mode,
   compiles each program with `loom build`, runs it N times (default 5), reports the
   median and min in milliseconds; with `--interp` also times `loom run --interp` (once per
   program, with a 60-second limit per run, reporting `timeout` beyond it).
3. Output is a Markdown table (program, native median, native min, interpreter, ratio)
   that can be pasted into docs.
4. The checksum printed natively must equal the interpreter's; a mismatch fails the run.
5. `cargo xtask bench` is not part of `cargo xtask ci`.

## Acceptance
- `cargo xtask bench --runs 3` works on macOS and Linux and prints the table.
- `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only the allowed paths.

## Notes
- Follow AGENTS.md. No new dependencies. Use only syntax from `spec/grammar.ebnf` and
  `examples/`; check every `.loom` file with `loom check` and `loom run`.
