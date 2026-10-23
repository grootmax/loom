# ADR 0003: Syntax philosophy

- **Status:** Accepted (provisional — revisited at the end of Phase 1, when the parser exists)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [spec §2–§4, §7](../../spec/loom-spec.md), [grammar.ebnf](../../spec/grammar.ebnf), [ADR 0004](0004-type-system-and-inference.md)

## Context

Loom's surface should read like Ruby: English-like keywords, `def … end`, blocks,
interpolation, keyword arguments. Ruby's flexibility (optional parentheses everywhere,
open classes, `method_missing`, several spellings of one thing) costs static tooling,
readability at scale, and error-message quality. We need Ruby's *feel* with a grammar a
hand-written parser, a formatter and a language server can handle precisely.

## Decision

**General shape**

1. UTF-8 source, `.loom` extension. Newlines terminate statements; there are no semicolons.
   Blocks close with `end`. Indentation is not significant; `loom fmt` normalizes it to 2 spaces.
2. A line continues onto the next when it ends with a binary operator, a comma, or an open
   bracket, or when the next line starts with `.` (method chains). Precise rules: spec §2.3.
3. `#` starts a line comment, and `##` starts a Markdown doc comment attached to the next item.
   Inside a string literal, `#{…}` is interpolation and `#` is not a comment.
4. Naming is enforced by the linter, not the parser: `snake_case` for values and functions,
   `PascalCase` for types, traits and enum variants, `SCREAMING_SNAKE_CASE` for constants.
   Modules are `snake_case`, matching their file names.

**Specific choices** (each one is "one obvious way")

| Topic | Decision | Rejected |
|---|---|---|
| Functions | `def name(params) -> Type … end`. Single expression: `def name -> Type = expr` | Ruby's optional return types everywhere |
| Return | The last expression is the value; `return` is used only for early exit | Mandatory `return` |
| Bindings | `x = 1` declares an immutable binding; `var x = 1` declares a reassignable one. Assigning to an immutable binding in scope is a compile error, not silent shadowing | Rust's `let` (noisy for Ruby readers); implicit shadowing (it causes the classic "closure updates a copy" bug) |
| Paths | `.` is the only separator: `std.fs.File`, `File.read(p)`, `Float.PI` | `::` for constants and namespaces (two spellings for one concept) |
| Logic | `and`, `or`, `not` | `&&`/`\|\|`/`!` (Ruby has both with different precedence, a known trap) |
| Ranges | `a..b` is half-open and `a..=b` is inclusive (Rust) | Ruby's `..`/`...`, which differ by one character and are routinely confused |
| Conditionals | `if … elsif … else … end`, and `if c then a else b end` inline. Pattern test: `if opt in Some(v)` | `unless`, the ternary `?:` (it conflicts with the `?` operator), statement modifiers like `x if y` |
| Matching | `case x in Pattern then … end`, with guards `in P if cond` | A separate `match` keyword; Ruby's value-based `case/when` |
| Loops | `for x in iter … end`, `while cond … end`, `loop do … end`; `break` and `next` work only in loops | `break`/`next` inside closures (they need non-local control flow) |
| Closures | `{ \|x\| expr }` (one line) and `do \|x\| … end` (multi-line), passed as a trailing block argument | Lambda arrow syntax |
| Calls | Parentheses are required, with two exceptions: zero-argument calls (`list.len`, uniform access with fields) and *command calls*. A command call is a statement-level call with a plain identifier callee whose first argument is a literal, identifier or `@field` (`puts "hi"`, `puts greet(u)`) | Ruby's paren-free calls anywhere (ambiguities like `foo -1`, `foo [1]`) |
| Methods | Inherent methods are written *inside* the `struct`/`enum` body. `impl Trait for Type … end` is used only for traits | Rust's separate inherent `impl` blocks (a Ruby reader expects methods in the type); open classes (rejected outright) |
| Receiver | Inside methods, `self` is implicit for `@field` (`@name` means `self.name`). Associated functions are written `def self.new(…)` | Explicit `self` parameters |
| Traits | Required methods are body-less signatures. Methods with a default body are prefixed `default def` | Unmarked default bodies: a body-less `def` followed by a trait's `end` is ambiguous with an empty-bodied method |
| Attributes | `@[derive(Eq, Hash)]`, `@[test]` (Crystal's syntax) | `#[…]`, which collides with comments |
| Unit | Type `Unit`, value `()`; functions with no `->` return `Unit` | Bare `()` as a type name |

**Metaprogramming:** none at runtime. Compile-time mechanisms are limited to built-in
`derive` attributes. User macros are not considered before 1.0, and only via RFC after it.

## What Rust does

Rust uses braces, semicolons, `let`, `::` paths, `&&`/`||`, `match`, mandatory parentheses,
`#[attr]`, and separate `impl` blocks. Loom keeps Rust's *structure*: expression-oriented
code, exhaustive `match` semantics, traits, attributes, half-open ranges, and no
runtime reflection-based metaprogramming. It changes the *spelling* toward Ruby. Every
deviation from Ruby above exists because Ruby's version is ambiguous for a static parser,
or because Ruby offers two ways to do one thing.

## Alternatives considered

- **Significant indentation (Python, Nim):** rejected. Copy-paste fragility, worse error
  recovery, and the kickoff mandates `end`.
- **Braces (Rust, Go, Swift):** familiar and unambiguous, but they abandon the Ruby feel
  that is a core goal.
- **Full Ruby call syntax:** rejected for the ambiguities listed above. The command-call
  subset keeps `puts x`, the most common case.

## Consequences

- Some Ruby idioms will not parse (`x if y`, `unless`, `&&`). Diagnostics must recognize
  them and suggest the Loom spelling, for example "Loom uses `not` instead of `!`".
- Uniform access (`user.name` can be a field or a zero-argument method) means name
  resolution, not parsing, decides which it is. Changing a field to a method is not a breaking change.
- Writing inherent methods inside the type body means a type's methods live in one file,
  which is what "no open classes" requires.
- The `default def` marker departs from the kickoff sample syntax (which showed only
  required methods). Recorded here so the change is visible.
