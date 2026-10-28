# Task 0021: Rich diagnostics in the CLI

## Context
`loom_diagnostics::render::render(&Diagnostic, &SourceMap, &RenderOptions) -> String`
renders rustc-style snippets (task 0009, merged). The CLI (`compiler/loom_cli/src/main.rs`)
still prints one-line `Diagnostic::short` output for `check`, `run`, `test` and `fmt`.
`--message-format=short` must keep printing exactly the one-line form, because
`cargo xtask conformance` parses it. Read AGENTS.md and `render.rs` first.

## Goal
`loom check`, `run`, `test` and `fmt` print rendered diagnostics by default, with a
summary line, and keep `--message-format=short` byte-for-byte unchanged.

## Files you may change
- `compiler/loom_cli/src/main.rs` (the functions that print diagnostics and the option
  parsing of `check`, `run`, `test`, `fmt`; add a small module if useful)
- new files under `compiler/loom_cli/tests/`

## Files you must NOT change
- every other file, including `render.rs`, `xtask/`, `spec/`

## Requirements
1. Default output: each diagnostic rendered with `render`, separated by a blank line, then
   a summary line `error: could not check `<file>` due to N previous errors` (or
   `… N previous errors; M warnings emitted`), matching rustc's wording style.
2. Color: enable only when stderr is a terminal and the environment variable `NO_COLOR` is
   unset (use `std::io::IsTerminal`); `--color=always|never|auto` overrides.
3. `--message-format=short` is accepted by `check`, `run` and `test` and prints exactly
   what it prints today.
4. Tests (`tests/cli_output.rs`, run the built binary via `env!("CARGO_BIN_EXE_loom")`):
   short format unchanged for a lexical error, a type error and an ownership error;
   default format contains the snippet line and the caret; `NO_COLOR` disables escape
   codes; the summary line counts errors and warnings.

## Acceptance
- `cargo test -p loom_cli` passes; `cargo xtask ci` passes (conformance still passes).
- `git diff --stat origin/main` shows only the allowed paths.

## Notes
- Follow AGENTS.md. No new dependencies. Never weaken or delete existing tests.
- Conventional Commit title: `feat(cli): render diagnostics with snippets by default`.
