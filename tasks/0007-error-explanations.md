# Task 0007: Long explanations for lexical and syntax error codes

## Context
Every Loom error code has a long explanation in
`compiler/loom_diagnostics/explanations/EXXXX.md`, shown by `loom explain EXXXX` (ADR 0008
§6). Rust's error index (`rustc --explain E0308`) is the model. Spec Appendix C (being added
to the spec in parallel, reproduced below) lists the codes for Phase 1. Spec §2 (lexical
structure, in `spec/loom-spec.md`) is complete, and `spec/grammar.ebnf` plus `examples/`
show the syntax. Read AGENTS.md, spec §2, the grammar and a few examples first.

## Goal
One accurate, friendly Markdown explanation per code E0001–E0027 and E0100–E0113.

## Files you may change
- new files under `compiler/loom_diagnostics/explanations/` only

## Files you must NOT change
- everything else (in particular `compiler/loom_diagnostics/src/`, `Cargo.toml`, `spec/`)

## Codes
| Code | Error | Spec |
|---|---|---|
| E0001 | Source file is not valid UTF-8 | §2.1.1 |
| E0002 | Bidirectional formatting character | §2.1.4 |
| E0003 | Carriage return not followed by line feed | §2.2.1 |
| E0004 | Non-ASCII whitespace character | §2.2.2 |
| E0005 | Unexpected character | §2 |
| E0006 | `;` is not used in Loom | §2.3.4 |
| E0007 | `!` is not an operator | §2.8.3 |
| E0008 | `&&` or `\|\|` is not an operator | §2.8.4 |
| E0009 | Reserved word used as an identifier | §2.6 |
| E0010 | Misplaced `_` in a numeric literal | §2.7.1 |
| E0011 | Decimal literal with a leading zero | §2.7.1 |
| E0012 | Invalid or missing digits in a numeric literal | §2.7.1, §2.7.2 |
| E0013 | Uppercase radix prefix | §2.7.1 |
| E0014 | Invalid suffix on a numeric literal | §2.7.1 |
| E0015 | Empty character literal | §2.7.3 |
| E0016 | Character literal with more than one character | §2.7.3 |
| E0017 | Unterminated character literal | §2.7.3 |
| E0018 | Unterminated string literal | §2.7.4 |
| E0019 | Unknown escape sequence | §2.7.5 |
| E0020 | Invalid `\u{…}` escape | §2.7.5 |
| E0021 | Invalid `\x` escape | §2.7.5 |
| E0022 | Non-ASCII character in a byte string | §2.7.4 |
| E0023 | Opening `"""` not followed by a line break | §2.7.4 |
| E0024 | Closing `"""` not on its own line | §2.7.4 |
| E0025 | Line in a triple-quoted string lacks the indentation prefix | §2.7.4 |
| E0026 | Unterminated raw string | §2.7.4 |
| E0027 | `#![` at the start of a file is reserved | §2.1.3 |
| E0100 | Expected a different token (general syntax error) | grammar |
| E0101 | Unclosed delimiter `(`, `[`, `{` or `#{` | §2.3 |
| E0102 | Missing `end` | §3, §7 |
| E0103 | Unexpected closing delimiter or `end` | §3, §7 |
| E0104 | Comparison operators cannot be chained (`a < b < c`) | §7.5 |
| E0105 | Range operators cannot be chained (`a..b..c`) | §7.5 |
| E0106 | Assignment is only allowed as a statement (`if x = 1`) | §4.2, §7.5 |
| E0107 | Positional argument after a named argument | §3.1 |
| E0108 | Inner doc comment (`##!`) after an item | §2.4 |
| E0109 | `impl` without a trait; inherent methods belong in the type's body | §3.5 |
| E0110 | `else if` is written `elsif` | §7.6 |
| E0111 | Two statements on one line | §2.3 |
| E0112 | Nesting too deep | — (implementation limit: 256 levels) |
| E0113 | Glob imports are not supported (`use a.*`) | §3.7 |

## Requirements
1. One file per code, named exactly `E0001.md` … `E0027.md`, `E0100.md` … `E0113.md`
   (41 files), plus `README.md` describing the format (requirement 2) in a few lines.
2. **Format** (follow exactly):
   ```markdown
   # E0007: `!` is not an operator

   One or two short paragraphs: what the error means and why Loom has this rule.

   ## Example

   ```loom
   <smallest program that produces this error>
   ```

   ## How to fix it

   One sentence, then:

   ```loom
   <the corrected program>
   ```

   See spec §2.8.3.
   ```
   The heading text after `EXXXX: ` is the "Error" column above, unchanged. Use real
   fenced code blocks with the `loom` info string. Each example program must produce
   **that** error and no other. Each fixed program must be valid Loom according to the spec
   and grammar.
3. **Style:** plain, friendly, second person ("You wrote…", "Use `not` instead"), no
   jargon without explanation, and no blame. Explain the *why* where the spec gives one. For
   example, E0002 explains "Trojan Source", E0007 explains that `not` reads like English,
   and E0109 explains that methods live in the type body so that a type's methods sit in
   one place (ADR 0003). Each file is at most ~40 lines.
4. Where Loom differs from Ruby or Rust, a one-line "Coming from Ruby/Rust" note is welcome
   (`&&` → `and`, `else if` → `elsif`, `impl Foo` → methods inside `struct Foo`, `;` → a
   line break).
5. Don't invent language features. If you aren't sure that a fixed example is valid,
   simplify it.

## Acceptance
- `cargo xtask ci` passes (unaffected).
- `ls compiler/loom_diagnostics/explanations/E*.md | wc -l` prints `41`.
- `git diff --stat origin/main` shows only files under `compiler/loom_diagnostics/explanations/`.

## Out of scope
- The `loom explain` command and embedding the files in the binary (a later task).

## Notes
- Follow AGENTS.md. No new dependencies.
- Conventional Commit title: `docs(diagnostics): add explanations for lexical and syntax errors`.
