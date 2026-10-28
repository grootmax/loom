# Task 0011: CST snapshot tests for every grammar production and every syntax error

## Context
`compiler/loom_parser` parses Loom into a lossless CST (`loom_parser::parse(text,
FileId::DETACHED)` returns a `Parse`; `parse.syntax().debug_dump()` prints the tree, and
`parse.diagnostics()` lists errors). `compiler/loom_snapshot` provides `assert_snapshot!`
(read its crate docs). ADR 0018 §8 requires CST dumps to be snapshot-tested for every
grammar rule. The grammar is `spec/grammar.ebnf`, and the syntax-error codes are spec
Appendix C.2 (E0100–E0113). Read AGENTS.md, the grammar and `compiler/loom_parser/src/`
first.

## Goal
`compiler/loom_parser/tests/` snapshot-tests the CST for every production in
`spec/grammar.ebnf` and the CST plus diagnostics for every syntax error code, so that any
change to the tree shape is visible in review.

## Files you may change
- new files under `compiler/loom_parser/tests/` (test files and `tests/snapshots/**`)
- `compiler/loom_parser/Cargo.toml`: only to add
  `loom_snapshot = { path = "../loom_snapshot" }` under `[dev-dependencies]`
  (keep the existing entries)
- `Cargo.lock`: only the resulting change

## Files you must NOT change
- `compiler/loom_parser/src/**` (if you find a parser bug, do not fix it: list it in the
  PR description, under "Suspected parser bugs", with the input and why the tree or
  diagnostic is wrong per the spec), `spec/`, everything else

## Requirements
1. A helper in `tests/common/mod.rs`, or at the top of each test file, that parses a
   source string and renders `debug_dump()`, followed by a line `--- diagnostics ---` and
   one line per diagnostic, `{id} {range} {message}` (omit the section if there are none).
2. One `#[test]` per case. The snapshot name is `<area>/<case>`, for example
   `items/fn_single_expression` or `errors/e0104_chained_comparison`. Areas: `items`,
   `statements`, `expressions`, `patterns`, `types`, `strings`, `newlines`, `errors`.
3. **Coverage:** at least one case for every production in `spec/grammar.ebnf` from
   `source_file` through `range_pattern` (the lexical productions are covered by lexer
   tests). Also cover: each precedence level of spec §7.5 (including that `not a == b` is
   `not (a == b)` and that `-a * b` is `(-a) * b`), command calls with and without a
   trailing block, trailing blocks with `{}` and `do … end`, leading-dot chains over
   blank and comment lines, newlines ignored inside brackets, doc comments attached to
   items, and nested string interpolation.
4. **Errors:** at least one case per code E0100–E0113, plus recovery cases showing that
   parsing continues correctly after the error (the items after the error parse
   normally). For E0112, nest 300 parentheses, built in the test with `"(".repeat(300)`.
5. Every snapshot file is committed (generate them with `LOOM_BLESS=1 cargo test -p loom_parser`).
   **Read every generated snapshot.** Check that the tree matches the spec (precedence,
   associativity, which node owns which tokens) and report anything that looks wrong under
   "Suspected parser bugs" instead of blessing it silently.
6. Keep each case's source small (1–10 lines).

## Acceptance
- `cargo xtask ci` passes.
- `cargo test -p loom_parser` runs at least 80 snapshot tests.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- Changing the parser.
- Lexer snapshots.

## Notes
- Follow AGENTS.md. This task may exceed 400 lines because snapshot files are generated;
  the hand-written test code should still be compact.
- Conventional Commit title: `test(parser): snapshot every grammar production and syntax error`.
