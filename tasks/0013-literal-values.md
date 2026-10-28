# Task 0013: Literal values in `loom_ast` (numbers, chars, strings, escapes)

## Context
`compiler/loom_ast` has typed views over the CST (read `src/lib.rs` and
`src/expressions.rs`): `Literal` (token kinds `INT_LIT`, `FLOAT_LIT`, `CHAR_LIT`,
`RAW_STRING_LIT`, `BYTE_STRING_LIT`, `TRUE_KW`, `FALSE_KW`) and `StringLit` (an interpolated
string: `STRING_START`, `STRING_FRAGMENT`s, `Interpolation`s, `STRING_END`). The fragments
hold *raw* source text. Phase 2's interpreter needs the values. Spec §2.7 (literals) and
§2.7.5 (escapes) define them exactly. Read AGENTS.md and spec §2.7 carefully.

## Goal
Accessors that compute the value of every literal exactly as spec §2.7 defines it.

## Files you may change
- new file `compiler/loom_ast/src/literals.rs`
- `compiler/loom_ast/src/lib.rs`: only add `mod literals;` and `pub use literals::*;`
- new files under `compiler/loom_ast/tests/`

## Files you must NOT change
- `compiler/loom_ast/src/expressions.rs` and every other file (add methods through
  `impl Literal { … }` / `impl StringLit { … }` blocks in `literals.rs`), `spec/`,
  everything else

## Requirements
1. Public API (exact names):
   ```rust
   pub enum LiteralValue { Int(u128), Float(f64), Char(char), String(String), Bytes(Vec<u8>), Bool(bool) }
   impl Literal { pub fn value(&self) -> Option<LiteralValue> }
   pub enum StringValuePart { Text(String), Interpolation(Interpolation) }
   impl StringLit { pub fn value_parts(&self) -> Vec<StringValuePart> }
   pub fn int_value(text: &str) -> Option<u128>
   pub fn float_value(text: &str) -> Option<f64>
   ```
   `value()` returns `None` if the literal is malformed. The lexer has already reported
   it, so no diagnostics here. Integers that don't fit in `u128` give `None`; range
   checks against a type are Phase 2's job.
2. Integers: decimal, `0x`, `0o`, `0b`, with `_` separators (§2.7.1). Floats: decimal with
   `_`, `.` and exponent (§2.7.2). Parse with `str::parse::<f64>` after removing `_`. A
   value that overflows to infinity gives `None`.
3. Escapes (§2.7.5): `\n \r \t \0 \\ \" \' \#`, `\xHH` (≤ 0x7F in strings and chars, any
   byte in byte strings), `\u{H…}` (1–6 hex digits, a scalar value), and `\` followed by a
   line terminator, which removes the terminator and the next line's leading spaces and
   tabs.
4. Ordinary strings: `value_parts` decodes each fragment and merges adjacent text. CR LF
   inside a string becomes `\n` (§2.7.4 rule 1).
5. **Triple-quoted strings** (§2.7.4 rule 3): remove the line terminator right after the
   opening `"""`. The indentation prefix is the whitespace before the closing `"""` on its
   line: remove it from the start of every content line (lines that are only whitespace
   become empty), and remove the line terminator before the closing line. Escapes and
   interpolations work as in ordinary strings. The prefix is removed from the *source*
   lines before escapes are processed. A line that starts inside an interpolation is not
   touched.
6. Raw strings `r"…"`, `r#"…"#`: the content between the delimiters, unchanged, with CR LF
   normalized to `\n`. Byte strings `b"…"` and `br"…"` give `LiteralValue::Bytes`. Char
   literals give `Char`. `true` and `false` give `Bool`.
7. **Tests** (`tests/literals.rs`, table-driven, parsing real Loom snippets with
   `loom_parser::parse`; `loom_parser` is already a dev-dependency of `loom_ast`):
   every radix and separator form; floats with exponents; every escape kind; line
   continuation; CRLF inside strings; raw strings with hashes; byte strings with `\xFF`;
   chars, including `'\u{1F600}'`; triple-quoted strings with nested indentation, blank
   lines and interpolation. Include the exact `poem` from `examples/04_strings.loom`: its
   value must make the program print exactly what `examples/04_strings.stdout` shows
   (lines 5–6 of that file, with the interpolated part as an `Interpolation` part).
8. Doc comments, clippy pedantic clean, no dependencies.

## Acceptance
- `cargo xtask ci` passes.
- `git diff --stat origin/main` shows only the allowed paths.

## Out of scope
- Type-dependent checks (literal fits `I8`, and so on) and diagnostics.

## Notes
- Follow AGENTS.md. Never weaken or delete existing tests.
- Conventional Commit title: `feat(ast): compute literal and string values per spec §2.7`.
