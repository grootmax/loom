# Task 0008: Conformance tests for syntax (spec §3–§11 grammar) and syntax errors

## Context
`spec/conformance/` holds implementation-independent tests (read
`spec/conformance/README.md`). The syntax is defined by `spec/grammar.ebnf` with the rules
in `spec/loom-spec.md` §3–§11, and illustrated by the 28 programs in `examples/`. No parser
exists in `main` yet. These tests specify its behavior in advance. Read AGENTS.md, the
grammar, spec §2.3 (newlines), §3, §4, §7 and the examples first. **The spec is the source
of truth.** If something here seems to contradict it, follow the spec and say so in the
PR description.

Clarifications being added to the spec in parallel (treat them as normative):
- **Command calls (§4.4):** a statement `IDENT arg, …` without parentheses is a call when
  the IDENT is followed by whitespace (not a line break) and then by a literal, a string,
  an IDENT, an `@field`, `self`, `Self` or a label `name:`. `puts -1` is a subtraction.
- **Trailing blocks (§7.3):** `{ … }` or `do … end` after an expression is a trailing block
  only on the same line. Its `|params|` are optional. A `{` that starts an operand is a
  closure if followed by `|`, and a map literal otherwise.
- **Closure parameters (§7.9):** each parameter is a single pattern atom, so or-patterns
  need parentheses: `{ |(A | B)| … }`.
- **`select` arms (§11.3):** `when P = e` if a `=` appears outside brackets before `then` or
  the end of the line; otherwise `when e`.

## Goal
Accept cases that cover every grammar construct, and one or more reject cases for each
syntax error code below. They live in per-section directories:
`spec/conformance/03-items/`, `04-statements/`, `07-expressions/`, `09-patterns/`.

## Files you may change
- new files under `spec/conformance/03-items/`, `spec/conformance/04-statements/`,
  `spec/conformance/07-expressions/`, `spec/conformance/09-patterns/`

## Files you must NOT change
- everything else, including `spec/loom-spec.md`, `spec/grammar.ebnf`, the conformance
  README and `spec/conformance/02-lexical/` (another task writes that directory)

## Format (normative for this task)
- Each case is `<name>.loom` plus **either** `<name>.stdout` (accept) **or** `<name>.diag`
  (reject). Names are `snake_case`, prefixed with the section, for example
  `7_5_chained_comparison.loom`.
- Line 1 of every `.loom` file is `# spec: §7.5` (plus `rule N` where applicable).
- **Accept cases** are complete programs with `def main … end` that print with `puts`. The
  `.stdout` file holds the exact output, ending with `\n`. Print only `Int`, `String` and
  `Bool` values (never Float or Char). Use only the language from the spec, grammar and
  examples. No standard-library calls except `puts`. Define your own structs, enums and
  functions as needed.
- **Reject cases:** the `.diag` file lists every expected diagnostic, one per line, as
  `LINE:COL CODE` (1-based; COL counts Unicode scalar values), in source order, ending with
  `\n`. Each reject case contains **exactly one** error and is otherwise valid, so the file
  has one line.

## Syntax error codes (spec Appendix C.2) and where each one points
| Code | Error | At |
|---|---|---|
| E0100 | Expected a different token (general syntax error) | the unexpected token; if a token is missing at the end of a line, the position of the line terminator (just past the last character) |
| E0101 | Unclosed delimiter `(`, `[`, `{` or `#{` | the opening delimiter |
| E0102 | Missing `end` | the keyword that opened the block (`def`, `if`, `struct`, …) |
| E0103 | Unexpected closing delimiter or `end` | the delimiter or `end` |
| E0104 | Comparison operators cannot be chained | the second operator |
| E0105 | Range operators cannot be chained | the second operator |
| E0106 | Assignment is only allowed as a statement | the `=` or compound operator |
| E0107 | Positional argument after a named argument | the start of the positional argument |
| E0108 | Inner doc comment (`##!`) after an item | the `##!` |
| E0109 | `impl` without a trait | the `impl` keyword |
| E0110 | `else if` is written `elsif` | the `if` |
| E0111 | Two statements on one line | the start of the second statement |
| E0113 | Glob imports are not supported | the `*` |

How an implementation recovers, so a case produces no extra errors:
- A missing closing delimiter always reports E0101 at the opener (never E0100). Inside
  brackets, line breaks are ignored, so an unclosed `(` swallows the rest of the file. Put
  such a case at the end of the file, as the value of a top-level
  `const X: Int = (1 + 2` with nothing after it.
- E0102: the block is the last thing in the file and simply never closed, for example
  `def main` / `  puts 1` / end of file.
- E0103: a stray `end` at top level after a complete item, or a stray `)` at the end of an
  otherwise complete statement such as `  puts 1)`.
- E0106: the parser reports the `=` and then parses the right-hand side normally, so
  `if a = b` … `end` gives only E0106.
- E0110: `else if c` is reported and then treated exactly like `elsif c` (no extra `end`
  needed).
- E0109: the members are parsed normally up to `end`.
- E0111: `x = 1 y = 2` reports at `y`. Both statements are then parsed.
- `..10` at the start of a statement (spec §2.3 example) is E0100 at the `..`.

## Requirements
1. **Reject cases:** at least one per code above (E0100 needs at least five different
   situations, for example a missing function name, `struct` with no name, a missing `in`
   in `for`, a missing `then` or line break after an `if` condition, and `..10` at the
   start of a statement). No case for E0112 (the nesting limit is implementation-defined).
2. **Accept cases:** cover every rule of `spec/grammar.ebnf` from `source_file` to
   `range_pattern` that can be exercised without the standard library. This includes
   attributes, visibility, `def` with and without parameters, generics, `where`, the
   single-expression form, `def self.new`, `def mut`, `own` parameters, default parameter
   values, keyword arguments and `name:` shorthand, structs, enums with every variant form,
   traits with required and `default def` methods and associated types, `impl … for`,
   inline `module` with `use path.{a, b as c}`, `const`, `type` aliases, every statement
   form (bindings with and without types and `var`, assignment to fields and indexes,
   compound assignment, command calls with labels and with a trailing block), every
   operator at every precedence level (spec §7.5; test that `not a == b` means
   `not (a == b)`, that `-2 * 3`, `1 + 2 * 3`, `1 << 2 + 1`, and `a or b and c` group
   correctly, by printing the results), `if`/`elsif`/`else` (block and `then` forms, and
   as an expression), `case` with guards, or-patterns, tuple, struct, range and literal
   patterns, `else` arms, `if x in P`, `while x in P`, `loop do … end` with `break`
   value, `for` with tuple patterns, `next`, `return`, closures `{ |x| … }`, `do |x| … end`,
   trailing blocks with and without params, tuples and `t.0.1`, array and map literals
   (print only their elements through your own code, or use them only in `for` loops over
   arrays, because no std methods are specified), `?` (with `Result`, `Ok` and `Err` as in `examples/10_result.loom`), and
   multi-line constructs inside brackets (newlines ignored, §2.3 rule 2).
   Where printing a value would need std methods, restructure the test so that it prints an
   `Int` computed by your own code.
3. Add a `README.md` in each of the four directories, with a table: case → rule →
   accept or reject.
4. Keep the total under ~150 files. Each case tests one clear thing.

## Acceptance
- `cargo xtask ci` passes (unaffected).
- `git diff --stat origin/main` shows only files under the four allowed directories.
- Every `.loom` file has exactly one matching `.stdout` or `.diag`. Every `.diag` line
  matches `^[0-9]+:[0-9]+ E[0-9]{4}$`. Include in the PR description the command you used
  to check this.

## Out of scope
- Lexical-error cases (E0001–E0099; another task), a parser, a test runner.

## Notes
- Follow AGENTS.md. No new dependencies.
- Conventional Commit title: `test(conformance): add syntax cases for spec §3–§11`.
