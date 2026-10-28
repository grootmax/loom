# Loom style (what `loom fmt` does)

`loom fmt` has no options ([ADR 0018] §10). There is one style, so code looks the same in
every project and nobody argues about it. This page is the contract the formatter is tested
against.

## Guarantees

1. **Meaning never changes.** The formatter only changes whitespace and line breaks. The
   sequence of tokens (other than whitespace) is identical before and after.
2. **Idempotent.** Formatting already-formatted code changes nothing.
3. **Comments are kept** where they are, with their text unchanged (trailing whitespace
   removed).
4. **Files with syntax errors are not formatted.** `loom fmt` prints the diagnostics and
   exits with status 1, leaving the file untouched.

## Rules

### Lines

- Line breaks are LF. The file ends with exactly one line break and has no blank lines at
  the start or end.
- No line ends in whitespace.
- At most one blank line in a row. No blank line directly after the line that opens a
  body (`def`, `struct`, `do`, `{`, …) or directly before the line that closes it (`end`,
  `}`, `)`, `]`).
- Your own line breaks are otherwise kept. **Phase 1 does not re-wrap long lines.** The
  100-column target is advisory until a later formatter version breaks long lines.

### Indentation

Two spaces per level. A line is indented one level for each of the following that
contains it and was opened on an earlier line:

- a body: of `def`, `if`/`elsif`/`else`, `case` arms, `while`, `for`, `loop`, `select`
  arms, `do … end` and `{ … }` blocks, and the member lists of `struct`, `enum`, `trait`,
  `impl`, `module` and `extern`;
- a bracket: `( )`, `[ ]`, `{ }` (maps), and `#{ }` in strings.

Several brackets opened on the same line count as one level. A line that starts with the
closing token (`end`, `)`, `]`, `}`) is at the level of the line that opened it. `elsif`,
`else`, `in` (case arms), `when` (select arms) and `end` line up with the keyword that
opened the construct:

```loom
def describe(shape: Shape) -> String
  case shape
  in Circle(radius:) then "circle of #{radius}"
  in Rect(width:, height:)
    area = width * height
    "rect of area #{area}"
  end
end
```

A **continuation line**, which continues a statement because the previous line ended with
an operator or because this line starts with `.` (spec §2.3), is indented one extra level.
Lines inside brackets are not indented again for continuation.

```loom
total = price +
  tax
names = users
  .filter { |u| u.active }
  .map { |u| u.name }
```

A comment on a line of its own is indented like the next line of code. If that line closes
a body (`end`, `}`), the comment is indented like the body.

The text inside string literals (including triple-quoted strings) is never changed.

### Spaces within a line

One space between tokens, except:

| No space | Example |
|---|---|
| after `(` `[` `@[` and before `)` `]` | `f(a, b)`, `[1, 2]`, `@[test]` |
| before `,` `:` `?` | `Point(x: 1, y: 2)`, `parse(text)?` |
| around `.` | `user.name`, `def self.new` |
| before the `(` of a call, parameter list, attribute arguments, variant fields, tuple-struct pattern or `Fn` type | `greet("Asha")`, `def f(x: Int)`, `Fn(Int) -> Int` |
| before the `[` of generics and indexing | `Array[T]`, `items[0]` |
| around `..` and `..=` | `1..=10`, `0..n` |
| after unary `-` and `~` | `-x`, `~mask` |
| inside the braces of a map literal and a `use` list | `{"a" => 1}`, `use std.time.{Time, Instant}` |
| inside closure parameter bars | `{ |x, y| x + y }`, `{ || 42 }` |

Closure and block braces have a space on both sides of their contents: `{ |x| x * 2 }`.

Before a comment at the end of a line, the spaces you wrote are kept (at least one), so
aligned trailing comments stay aligned.

[ADR 0018]: ../adr/0018-front-end-design.md
