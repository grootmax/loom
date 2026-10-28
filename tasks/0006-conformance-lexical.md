# Task 0006: Conformance tests for spec §2 (lexical structure)

## Context
`spec/conformance/` holds implementation-independent tests (read
`spec/conformance/README.md`). Spec §2 (lexical structure) in `spec/loom-spec.md` is
complete and normative. No lexer exists yet in `main`. These tests specify its behavior in
advance, and a runner (`cargo xtask conformance`, a later task) will execute them. Read
AGENTS.md and spec §2 carefully first. **The spec is the source of truth.** If anything
here seems to contradict it, follow the spec and say so in the PR description.

## Goal
`spec/conformance/02-lexical/` has an accept case and/or a reject case for every numbered
rule of spec §2 where that makes sense, following the format below exactly.

## Files you may change
- new files under `spec/conformance/02-lexical/` only

## Files you must NOT change
- everything else, including `spec/loom-spec.md`, `spec/grammar.ebnf`,
  `spec/conformance/README.md` and any other directory under `spec/conformance/`

## Format (normative for this task)
- Each case is `<name>.loom` plus **either** `<name>.stdout` (accept) **or** `<name>.diag`
  (reject). Names are `snake_case`, prefixed by the section, for example
  `2_7_1_leading_zero.loom`, `2_3_trailing_operator_continues.loom`.
- **Line 1** of every `.loom` file is `# spec: §2.7.1 rule 2` (the section and rule numbers
  the case tests). Exceptions: shebang/BOM cases, where line 1 must be the shebang or start
  with the BOM; put the `# spec:` comment on line 2.
- **Accept cases** are complete programs with `def main … end` that print using `puts` with
  one argument. The `.stdout` file holds the exact output, one line per `puts`, and ends
  with `\n`. Print only `Int`, `String` and `Bool` values. **Never print a Float or Char**
  (their display format isn't specified yet). `puts 0x1F` prints `31`; `puts true` prints
  `true`; `puts "a#{1 + 1}b"` prints `a2b`.
- **Reject cases**: the `.diag` file lists every expected diagnostic, one per line, as
  `LINE:COL CODE`, in source order, ending with `\n`. LINE and COL are 1-based. COL counts
  Unicode scalar values (not bytes) from the start of the line, and a tab is one column.
  The position is the one given in the "At" column of the code table below. Each reject
  case should contain **exactly one** error and be otherwise valid, so the file has
  exactly one line. Keep reject cases tiny, usually a `def main` with one statement,
  or a single top-level `const NAME: Type = …` item.

## How an implementation recovers (so a case produces no extra errors)
- An error token the lexer reports (an unexpected character like `$`) is ignored by the
  parser, as if it were whitespace. `x = 1 $` therefore gives only E0005.
- `;` gives E0006 and then acts as a line break, so `a = 1; b = 2` gives only E0006.
- `!` gives E0007 and parses as `not`; `&&` / `||` give E0008 and parse as `and` / `or`.
- Non-ASCII whitespace (E0004), lone CR (E0003), bidi characters (E0002) and `#![` (E0027)
  are reported and then treated as whitespace, a line break or comment text, so nothing
  else breaks.
- A reserved word (E0009) is lexed as an identifier, so `yield = 1` is otherwise a valid
  binding.
- Malformed numeric literals (E0010–E0014) still form one literal token. All letters and
  digits that directly follow are part of it, including an invalid suffix like `u8`.
- An unterminated string or raw string (E0018, E0026) runs to the end of the file. Put such
  a case at the very end, as the value of a top-level `const` (no enclosing `def`), so no
  `end` is missing. An unterminated char literal (E0017) ends at the end of its line.

## Diagnostic codes (spec Appendix C.1, being added to the spec in parallel)
| Code | Error | At |
|---|---|---|
| E0001 | Source file is not valid UTF-8 | first invalid byte |
| E0002 | Bidirectional formatting character (U+202A–U+202E, U+2066–U+2069) | the character |
| E0003 | Carriage return not followed by line feed | the CR |
| E0004 | Non-ASCII whitespace character | the character |
| E0005 | Unexpected character | the character |
| E0006 | `;` is not used in Loom | the `;` |
| E0007 | `!` is not an operator | the `!` |
| E0008 | `&&` or `\|\|` is not an operator | the operator |
| E0009 | Reserved word used as an identifier | the word |
| E0010 | Misplaced `_` in a numeric literal | the literal |
| E0011 | Decimal literal with a leading zero | the literal |
| E0012 | Invalid or missing digits in a numeric literal (`0x`, `0b102`, `0o9`) | the literal |
| E0013 | Uppercase radix prefix (`0X1F`) | the literal |
| E0014 | Invalid suffix on a numeric literal (`10u8`, `1.5x`) | the suffix |
| E0015 | Empty character literal | the literal |
| E0016 | Character literal with more than one character | the literal |
| E0017 | Unterminated character literal | the opening `'` |
| E0018 | Unterminated string literal | the opening delimiter |
| E0019 | Unknown escape sequence | the escape (its `\`) |
| E0020 | Invalid `\u{…}` escape (no digits, more than 6, surrogate, above 10FFFF) | the escape |
| E0021 | Invalid `\x` escape (non-hex digits; above 0x7F outside byte strings) | the escape |
| E0022 | Non-ASCII character in a byte string | the character |
| E0023 | Opening `"""` not followed by a line break | the `"""` |
| E0024 | Closing `"""` not on its own line | the closing `"""` |
| E0025 | Line in a triple-quoted string lacks the indentation prefix | start of the line (column 1) |
| E0026 | Unterminated raw string | the opening `r` or `br` |
| E0027 | `#![` at the start of a file is reserved | the `#` |

## Requirements
1. At least one **reject** case per code E0001–E0027. Several codes need more than one case
   to cover their rules. E0012 needs hex, octal and binary; E0020 needs each failure kind;
   E0021 needs both kinds; E0008 needs both `&&` and `||`.
2. **Accept** cases covering at least:
   - BOM ignored (§2.1.2); shebang ignored (§2.1.3);
   - CRLF line endings (§2.2.1). Commit the file with CRLF and add
     `spec/conformance/02-lexical/.gitattributes` with `*_crlf.loom -text` so Git keeps the bytes;
   - every continuation form of §2.3 rule 1 (a): trailing binary operator, `,`, `.`, `=`,
     compound assignment, `->` in a multi-line signature, `and` / `or` / `not`;
   - leading-dot method chains (rule 1 (b)), including blank and comment-only lines in
     between. No standard library is specified yet, so define your own `struct` whose
     methods return a new value of the struct, and print an `Int` field at the end;
   - a comment-only line inside an expression continued by a trailing operator;
   - comments and doc comments (§2.4);
   - Unicode identifiers such as `größe` and `名前`, `_`-prefixed bindings, and the
     contextual keyword `default` used as an ordinary identifier (§2.5, §2.6);
   - integer literals in every radix, with underscores (§2.7.1);
   - §2.7.2 rules 1–3 through tuple field access: `t = (1, (2, 3))`, then `puts t.1.0`
     prints `2`;
   - string escapes and interpolation, including nested interpolation, `\#{`, and the
     backslash-newline line continuation (§2.7.4, §2.7.5); triple-quoted strings with
     indentation stripping and interpolation; raw strings with `#` delimiters;
   - `{ || 42 }` (§2.8 rule 4), passed to a function that takes `f: Fn() -> Int` and
     returns `f()`.
3. Use only language features from the spec and `examples/` (functions, structs, enums,
   `case`, `if`, `for` over integer ranges, `puts`, string interpolation). Don't call
   standard-library methods other than `puts`, because none are specified yet.
4. E0001 needs a file containing an invalid UTF-8 byte. Create it with a byte-exact method
   (for example `printf`) and add `*_invalid_utf8.loom binary` to the same
   `.gitattributes`. E0002 and E0004 need the literal characters in the file. Write them
   byte-exact and make sure your editor doesn't strip them.
5. Add `spec/conformance/02-lexical/README.md` with a table: case name → rule → accept or
   reject. Every case must appear in it.
6. Keep the total under ~120 files. Quality over quantity: each case tests one thing,
   and its `# spec:` line says which.

## Acceptance
- `cargo xtask ci` passes (it doesn't read these files yet; it must still pass).
- `git diff --stat origin/main` shows only files under `spec/conformance/02-lexical/`.
- Every `.loom` file has a matching `.stdout` or `.diag` (not both). Every `.diag` line
  matches the regex `^[0-9]+:[0-9]+ E[0-9]{4}$`. State in the PR description the command
  you used to check this.

## Out of scope
- Writing a lexer or test runner.
- Syntax-error cases (E0100+); another task covers them.

## Notes
- Follow AGENTS.md. No new dependencies.
- Conventional Commit title: `test(conformance): add lexical structure cases for spec §2`.
