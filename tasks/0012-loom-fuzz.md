# Task 0012: `loom_fuzz`, an in-house std-only fuzzer for the front end

## Context
Phase 1's exit criterion includes "parser fuzzed 24 h clean" (docs/roadmap.md). ADR 0018
§9 decides on an in-house, std-only fuzzer, with no cargo-fuzz or libFuzzer for now. The
front-end APIs:
- `loom_lexer::lex(text, FileId::DETACHED) -> Lexed` (`tokens: Vec<Token { kind, range }>`,
  `diagnostics`);
- `loom_parser::parse(text, FileId::DETACHED) -> Parse` (`syntax()`, `diagnostics()`,
  `has_errors()`), and `loom_parser::parse_bytes(bytes, FileId::DETACHED)`;
- `loom_fmt::format_tree(&SyntaxNode) -> String` (only for inputs without errors).
Read AGENTS.md, ADR 0018 §9 and these crates' public APIs first.

## Goal
`cargo run --release -p loom_fuzz -- --target parse --seconds 60 --seed 1` hammers the
front end with mutated and generated inputs, checks the invariants below, and reports
minimized failing inputs reproducibly.

## Files you may change
- new directory `tools/loom_fuzz/` (Cargo.toml, `src/`, `tests/`, `corpus/regressions/`)
- `Cargo.toml` (root): **only** add `"tools/loom_fuzz",` to `members`, after `"tools/loom_pkg",`
- `Cargo.lock`: only the resulting change

## Files you must NOT change
- every other crate, `spec/`, `docs/adr/`, `.github/`, everything else

## Requirements
1. Crate `loom_fuzz` (copy the `[package]` layout of `tools/loom_fmt/Cargo.toml`;
   description `"In-house fuzzer for the Loom front end."`), a library plus a binary. Path
   dependencies only: `loom_span`, `loom_syntax`, `loom_lexer`, `loom_parser`, `loom_fmt`.
2. **CLI:** `loom_fuzz --target <lex|parse|fmt|all> [--seconds N] [--iterations N]
   [--seed S] [--corpus DIR]... [--out DIR] [--replay FILE]`. Defaults: `all`, 10 seconds,
   seed taken from the clock (always printed), corpus = `examples/` and `spec/conformance/`
   (recursively, `.loom` files) plus `tools/loom_fuzz/corpus/regressions/`, and
   `--out target/fuzz`. `--replay FILE` runs the targets once on that file and reports.
3. **Determinism:** a hand-written PRNG (splitmix64 or xorshift64*), so the same seed
   produces the same input sequence. Print `seed=S` at start and progress
   (`iteration N, inputs/sec R`) every 10 seconds, so an abort (which cannot be caught)
   can be reproduced with `--seed S --iterations N`.
4. **Input sources**, chosen at random each iteration:
   byte-level mutation of a corpus file (flip, insert, delete, duplicate a range);
   token-level mutation (insert, delete or replace a token from a dictionary of every
   keyword and punctuation from spec §2.6/§2.8 plus `"`, `"""`, `#{`, `}`, `\`, `'`,
   `r#"`, `b"`, `@[`, `##`, `\u{`, a newline, a tab, U+00A0 and U+202E); line-level
   mutation (duplicate, delete or swap lines); splicing two corpus files; nesting (wrap a
   random range in `(`…`)`, `[`…`]`, `do`…`end` or `if x`…`end`, sometimes 50+ times); and
   generation from a small recursive generator of random but mostly grammatical Loom
   (items, statements, expressions, with a depth limit). Also feed arbitrary bytes
   through `parse_bytes`.
5. **Invariants** (a violation is a failure):
   - *lex:* no panic; token ranges are contiguous, non-empty and cover the input; lexing
     the same input twice gives the same tokens.
   - *parse:* no panic; `syntax().text()` equals the input (equals the lossy decoding for
     `parse_bytes`); every diagnostic's range lies within the input; a single parse
     takes less than 2 seconds (report it as a failure, "slow input", if not).
   - *fmt:* for inputs that parse without errors, `format_tree` doesn't panic, its output
     parses without errors, formatting the output again gives the same output, and the
     non-whitespace tokens (kind and text, ignoring `WHITESPACE`, `LINE_BREAK` and `NEWLINE`
     and trailing whitespace of comments) are identical before and after.
6. **Failures:** catch panics with `std::panic::catch_unwind`, silencing the default hook
   while fuzzing. On a failure, minimize the input (remove chunks of decreasing size, then
   single characters, while the same invariant still fails), write it to
   `<out>/<target>-<short hash>.loom`, print the path and the invariant, and keep going.
   At the end, exit 1 if anything failed.
7. **Tests:** `tests/smoke.rs` runs each target for a fixed number of iterations (for
   example 500) with a fixed seed and asserts no failures, finishing in a few seconds in
   debug builds. Another test replays every file in `corpus/regressions/`, which starts
   out empty apart from a `README.md` that explains that minimized crashers go there.
   Unit tests for the PRNG's determinism and for the minimizer, using a fake predicate.
8. If the fuzzer finds real bugs in the lexer, parser or formatter while you develop it,
   **don't fix them**: list each one in the PR description with the minimized input. Commit
   the minimized inputs to `corpus/regressions/` **only** if the regression test is
   marked `#[ignore = "known bug: …"]` with the input name. The maintainer fixes the bugs
   and un-ignores them.
9. Doc comments, clippy pedantic clean, no `unsafe`, no dependencies.

## Acceptance
- `cargo xtask ci` passes.
- `cargo run --release -p loom_fuzz -- --seconds 60 --seed 1` runs and reports. Paste its
  summary line into the PR description.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- A CI workflow for long runs (a follow-up task), coverage guidance, and fixing
  front-end bugs.

## Notes
- Follow AGENTS.md. This task is pre-approved to exceed ~400 lines (up to ~800).
- Conventional Commit title: `feat(fuzz): add the in-house front-end fuzzer`.
