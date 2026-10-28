# The Loom Language Specification

- **Edition:** `preview` (pre-1.0; no compatibility guarantees yet)
- **Status:** Draft. §2 (Lexical structure) is complete enough to implement against;
  other chapters are outlines marked `TODO(Phase N)` with the rules already decided in ADRs.
- **Normative grammar:** [`grammar.ebnf`](grammar.ebnf). Every grammar rule cites the section
  of this document that defines its meaning.

This document is **normative**. Where an implementation disagrees with it, the
implementation is wrong, unless this document is amended through an ADR or RFC
([ADR 0001](../docs/adr/0001-record-decisions-as-adrs.md)).

---

## 1. Introduction

### 1.1 Scope

This specification defines the syntax and semantics of Loom programs: what text is a valid
program, and what a valid program does when run. It does not specify the toolchain's
command-line interface, diagnostics wording, or performance.

### 1.2 Notation

- Grammar productions use the ISO/IEC 14977-style EBNF described at the top of
  `grammar.ebnf`. Terminal strings are in double quotes; `{ x }` is zero or more,
  `[ x ]` is optional, `( a | b )` is a choice.
- **must**, **must not**, **may** and **should** have their RFC 2119 meanings.
- "It is an error" means a conforming implementation must reject the program at compile
  time with a diagnostic. "Panics" means a runtime panic (§10.4).
- Code samples are informative unless stated otherwise.

### 1.3 Conformance

A conforming implementation accepts every valid program, rejects every invalid program
with at least one diagnostic, and executes valid programs with the behavior specified
here. Behavior this document marks *unspecified* may vary between implementations, but
must not be undefined. The conformance suite in `spec/conformance/` is the executable
companion to this document.

### 1.4 Editions

Each package selects an edition ([ADR 0011](../docs/adr/0011-editions-and-compatibility.md)).
This document describes edition `preview`. Later editions will note their differences inline.

---

## 2. Lexical structure

The lexer converts source text into a sequence of **tokens** and **trivia** (whitespace
and comments). Trivia never affect meaning, except where §2.3 says a line terminator does.
A lossless representation of all tokens and trivia must be recoverable (it is used by
`loom fmt` and the language server), but that requirement is not part of the language semantics.

### 2.1 Source text

1. A source file is a sequence of Unicode scalar values encoded as **UTF-8**. A file that
   is not valid UTF-8 is an error.
2. A single U+FEFF BYTE ORDER MARK at the start of the file is ignored.
3. If the first line begins with `#!`, the whole first line is ignored (shebang), unless it
   begins with `#![`, which is reserved.
4. The characters U+202A–U+202E and U+2066–U+2069 (bidirectional formatting controls)
   are an error **anywhere** in a source file, including comments and string literals.
   Inside string literals they may be written with `\u{…}` escapes. *(Rationale: prevents
   "Trojan Source" attacks, where displayed code differs from compiled code.)*
5. Source files use the extension `.loom`.

### 2.2 Line terminators and whitespace

1. A **line terminator** is U+000A LINE FEED, or U+000D U+000A (CR LF), which counts as one
   terminator. A U+000D not followed by U+000A is an error.
2. **Whitespace** is U+0020 SPACE and U+0009 TAB. Any other Unicode whitespace character
   (for example U+00A0 NO-BREAK SPACE) outside a comment or string literal is an error,
   with a diagnostic that names the character.
3. Whitespace separates tokens and is otherwise insignificant. Indentation has no meaning.

### 2.3 Statement termination and line continuation

Loom has no semicolons. A line terminator ends a statement, subject to the rules below.
Each line terminator produces a `NEWLINE` token, unless it is *suppressed* by rule 1;
the parser then ignores `NEWLINE`s in the contexts of rule 2.

1. **Continuation (lexical).** A line terminator does not produce `NEWLINE` if:
   - (a) the last token before it, ignoring trivia, is one of: a binary operator from §2.8
     *other than* `|`; `,`; `.`; `=` or a compound assignment operator; `->`; `=>`; `and`;
     `or`; `not`; or
   - (b) the first token on the next line that is not inside a whitespace-only or
     comment-only line is `.` followed immediately by an identifier or `@` (a leading-dot
     method chain). A line beginning with `..` or `..=` does **not** continue the previous line.

   Lines containing only whitespace and/or comments between the two lines are skipped
   when applying rule (b).

   Because rule (a) ignores trivia, a comment-only or blank line inside a continued
   expression does not end it: every line terminator up to the next token is suppressed.
   A suppressed line terminator is trivia (`LINE_BREAK`, §2.9).
2. **Bracketed contexts (syntactic).** Inside a parenthesized expression, an argument or
   parameter list, an array literal, a map literal, an index expression, or a generic
   argument list, the parser ignores `NEWLINE` tokens. A nested **block body** (§7.9 closure
   bodies, and the bodies of `do`, `if`, `case`, `loop`, `while`, `for`, `select`) restores
   statement context, so `NEWLINE` separates statements inside it again.
3. Consecutive `NEWLINE` tokens are equivalent to one.
4. The token `;` is not part of Loom. It is an error, and implementations should suggest
   using a line break instead.

*Example (informative):*

```loom
total = price +          # continues: trailing binary operator
  tax
users
  .filter { |u| u.active }   # continues: leading `.`
  .count
r = 0
..10                       # does not continue: line starts with `..` (and is an error here)
```

### 2.4 Comments

1. A **line comment** starts with `#` (outside a string literal) and extends to the end of
   the line. It does not include the line terminator.
2. A comment starting with `##` (but not `##!`) is an **outer doc comment**. Consecutive
   outer doc comment lines form one Markdown document attached to the next item, field,
   variant or method. An outer doc comment not followed by such a declaration is a warning.
3. A comment starting with `##!` is an **inner doc comment**. It documents the enclosing
   module (or the file's module, at the top of a file). Inner doc comments may appear only
   before any item in their module.
4. There are no block comments.
5. Doc comments are trivia for parsing purposes, but their text is part of the program's
   documentation and is preserved by tools.

### 2.5 Identifiers

1. An **identifier** is a character in `XID_Start` or `_`, followed by any number of
   characters in `XID_Continue` (Unicode Standard Annex #31).
2. Identifiers are compared after Unicode Normalization Form C (NFC). `loom fmt` writes
   identifiers in NFC. Confusable identifiers in one crate (UTS #39 skeletons that are
   equal) produce a warning.
3. A keyword (§2.6) is not an identifier.
4. The identifier `_` alone is the **wildcard**. It is valid only as a pattern, a binding
   name that discards its value, or an inferred type argument.
5. An identifier beginning with `_` suppresses unused-binding warnings.
6. **Field reference:** `@` immediately followed (no whitespace) by an identifier forms an
   `AT_IDENT` token, for example `@name`. Its meaning is given in §7.4.
7. Identifiers must not end in `?` or `!`. *(Ruby allows them; in Loom, `?` is the
   propagation operator, §10.3. Predicates are spelled `is_empty`, `has_key`.)*
8. Naming conventions (snake_case, PascalCase, SCREAMING_SNAKE_CASE) are enforced by the
   linter, not by this specification.

### 2.6 Keywords

**Strict keywords** cannot be used as identifiers:

```
and      as       break    case     const    def      do       dyn
else     elsif    end      enum     extern   false    for      if
impl     in       loop     module   mut      next     not      or
own      package  pub      ref      return   select   self     Self
struct   super    then     trait    true     type     unsafe   use
var      when     where    while
```

**Contextual keywords** are identifiers except in the listed position:

| Word | Keyword when |
|---|---|
| `default` | immediately followed by `def` inside a `trait` body (§3.4) |

**Reserved words** are not used yet, but are reserved so that future editions can use them
without breaking code. Using one is an error:

```
abstract async await become box catch defer do_while final macro
override priv try typeof unsized virtual yield
```

### 2.7 Literals

#### 2.7.1 Integer literals

```
INT_LIT  = DEC_LIT | HEX_LIT | OCT_LIT | BIN_LIT
DEC_LIT  = "0" | NONZERO { ["_"] DIGIT }
HEX_LIT  = "0x" HEX_DIGIT { ["_"] HEX_DIGIT }
OCT_LIT  = "0o" OCT_DIGIT { ["_"] OCT_DIGIT }
BIN_LIT  = "0b" BIN_DIGIT { ["_"] BIN_DIGIT }
```

1. An underscore may appear only *between* two digits. `1__0`, `1_` and `0x_1` are errors.
2. A decimal literal with a leading zero followed by more digits (`012`) is an error,
   because it would be ambiguous with C-style octal. Use `0o12` or `12`.
3. Prefixes are lowercase only (`0X1` is an error).
4. Integer literals have no type suffixes. Their type is inferred from context and defaults
   to `Int` (§6). A literal that does not fit its type is an error.
5. There are no negative literals. `-5` is unary minus applied to `5`, but the constant
   `-9223372036854775808` is accepted for `I64`, as in Rust.

#### 2.7.2 Float literals

```
FLOAT_LIT = DEC_DIGITS "." DEC_DIGITS [ EXPONENT ]
          | DEC_DIGITS EXPONENT
EXPONENT  = ( "e" | "E" ) [ "+" | "-" ] DEC_DIGITS
```

1. A float literal needs digits on both sides of `.`. `1.` and `.5` are not float literals.
   So `10.seconds` is the integer `10`, then `.`, then the identifier `seconds`.
2. `1..2` lexes as `1`, `..`, `2`.
3. **After a `.` token, a numeric literal is always lexed as an integer**, never a float,
   so that tuple field access chains work: `t.0.1` is `t . 0 . 1`.
4. The type defaults to `Float` (= `F64`). A literal whose value overflows to infinity is an error.

#### 2.7.3 Character literals

1. A **character literal** is a single Unicode scalar value or escape (§2.7.5) between
   single quotes: `'a'`, `'\n'`, `'\u{1F600}'`. Its type is `Char`.
2. `''` and multi-character literals are errors. *(Unlike Ruby, single quotes are not strings.)*

#### 2.7.4 String literals

1. A **string literal** is delimited by `"` and may contain any characters except an
   unescaped `"` or `\`. It may span multiple lines; line terminators inside it are
   included as U+000A. Its type is `String` (§5.1).
2. **Interpolation:** inside a string literal, `#{` begins an embedded expression, which
   extends to the matching `}`. The embedded expression may itself contain string
   literals, braces and interpolations. The lexer tracks nesting. Interpolation is
   evaluated left to right, and each value is formatted with its `Display` implementation
   (§7.12). `#` not followed by `{` is an ordinary character.
3. **Triple-quoted strings** (`"""`) are for multi-line text:
   - the opening `"""` must be followed immediately by a line terminator, which is not
     part of the value;
   - the closing `"""` must be preceded on its line only by whitespace. That whitespace is
     the *indentation prefix*. It is removed from the start of every content line, and every
     non-blank content line must begin with it (an error otherwise). The line terminator
     before the closing line is not part of the value;
   - escapes and interpolation work as in ordinary strings.
4. **Raw strings** `r"…"`, `r#"…"#`, `r##"…"##`, … contain no escapes and no interpolation.
   They end at the first `"` followed by the same number of `#` as the opening.
5. **Byte strings** `b"…"` contain only ASCII characters and byte escapes (`\xHH` with any
   value 00–FF). They have type `Bytes`. `br"…"` is a raw byte string. Byte strings have no interpolation.

#### 2.7.5 Escapes

| Escape | Meaning |
|---|---|
| `\n` `\r` `\t` `\0` | LF, CR, TAB, NUL |
| `\\` `\"` `\'` | backslash, double quote, single quote |
| `\#` | a literal `#` (so `"\#{x}"` is the four characters `#{x}`) |
| `\xHH` | in strings and chars: a code point ≤ 0x7F; in byte strings: any byte |
| `\u{H…}` | a Unicode scalar value, 1–6 hex digits; surrogates are an error |
| `\` + line terminator | line continuation inside a string: the terminator and the next line's leading whitespace are removed |

Any other escape is an error.

#### 2.7.6 Boolean literals

`true` and `false`, of type `Bool`.

### 2.8 Operators and punctuation

```
Arithmetic   +  -  *  /  %
Bitwise      &  |  ^  ~  <<  >>
Comparison   ==  !=  <  <=  >  >=
Logical      and  or  not                (keywords)
Assignment   =  +=  -=  *=  /=  %=  &=  |=  ^=  <<=  >>=
Range        ..  ..=
Other        .  ,  :  ->  =>  ?  @  @[  (  )  [  ]  {  }
```

1. Tokens are formed by **maximal munch**: the longest sequence that forms a token is
   taken. `a..=b` is `a`, `..=`, `b`. Exceptions are §2.7.2 rules 2 and 3.
2. `|` also delimits closure parameters (§7.9). For that reason it is not a continuation
   token (§2.3).
3. `!` is not an operator; it appears only in `!=`. A bare `!` is an error, and the
   diagnostic should suggest `not` (or `~` for bitwise complement).
4. `&&` and `||` are errors, and the diagnostic should suggest `and` / `or`. **Exception:**
   `||` immediately after `{` or `do` (ignoring whitespace) is lexed as two `|` tokens, an
   empty closure parameter list (`{ || 42 }`).
5. Operator precedence and associativity are given in §7.5.

### 2.9 Token classes (summary)

`IDENT`, `AT_IDENT`, keywords, `INT_LIT`, `FLOAT_LIT`, `CHAR_LIT`, `STRING_START` /
`STRING_FRAGMENT` / `INTERP_START` / `INTERP_END` / `STRING_END` (an interpolated string is
a token sequence), `RAW_STRING_LIT`, `BYTE_STRING_LIT`, operators and punctuation,
`NEWLINE`, `EOF`. Trivia: `WHITESPACE`, `LINE_BREAK` (a line terminator suppressed by
§2.3 rule 1), `COMMENT`, `DOC_COMMENT`, `INNER_DOC_COMMENT`, `SHEBANG`.

---

## 3. Items

*TODO(Phase 1/2): full text. Rules already decided are listed.*

### 3.1 Functions

`def name[Generics](params) -> ReturnType … end`, or the single-expression form
`def name(params) -> T = expr`. Parameters have modes (§8.2): `x: T` (borrow), `mut x: T`,
`own x: T`. Keyword arguments: every parameter can be passed by name (`f(x: 1)`).
Positional arguments must precede named ones. A function with no `->` returns `Unit`.
The body's final expression is the return value.

### 3.2 Structs

`struct Name[Generics] … end` containing fields `name: Type` (each optionally `pub`) and
inherent methods (`def`). Construction is `Name(field: value, …)`, with all fields named.
The shorthand `Name(field:)` takes the value from a binding of the same name.

### 3.3 Enums

`enum Name[Generics] … end` containing variants `Variant`, `Variant(Type, …)` or
`Variant(field: Type, …)`, and inherent methods. Variants are referred to as
`Name.Variant`, or unqualified where the expected type is known.

### 3.4 Traits

`trait Name[Generics] [: Supertraits] … end` containing required method signatures
(`def m(params) -> T`, no body), provided methods (`default def m … end`), and associated
types (`type Item`). Body-less signatures appear only in traits.

### 3.5 Implementations

`impl[Generics] Trait for Type [where …] … end` implements a trait. Inherent methods are
written inside the type's own body (§3.2–3.3), never in `impl` blocks
([ADR 0003](../docs/adr/0003-syntax-philosophy.md)). Coherence follows Rust's orphan rule.

### 3.6 Modules

Each file is a module (§12). `module name … end` declares an inline module.

### 3.7 Use declarations

`use path`, `use path as alias`, `use path.{a, b as c}`. No glob imports.

### 3.8 Constants and statics

`const NAME: Type = expr`. The expression is evaluated at compile time.
*TODO(Phase 2):* the constant-evaluation subset.

### 3.9 Type aliases

`type Name[Generics] = Type`.

### 3.10 Attributes

`@[name]`, `@[name(args)]` precede an item, field, variant or method. Phase 0 set:
`derive(...)`, `test`, `inline`, `deprecated(...)`, `stable(...)`, `unstable(...)`.

### 3.11 Visibility

Private to the module by default. `pub` makes an item public; `pub(package)` limits it to the package.

## 4. Statements

*TODO(Phase 1/2).*

### 4.1 Bindings

`x = expr` introduces an immutable binding when `x` is not already bound in an enclosing
scope of the same function. `var x = expr` introduces a reassignable binding. A type may
be given: `x: T = expr`, `var x: T = expr`.

Syntactically, `x = expr` with a lone identifier on the left is a binding statement.
Name resolution reclassifies it as an assignment (§4.2) when `x` is a `var` binding that is
already in scope.

### 4.2 Assignment

`place = expr` and compound assignment, where `place` is a `var` binding, or a field or
index reached through a `var` binding or a `mut` parameter. Assigning to an immutable
binding that is in scope is an error. Bindings are never implicitly shadowed
([ADR 0003](../docs/adr/0003-syntax-philosophy.md)).

### 4.3 Expression statements

Any expression may be a statement. A non-`Unit` value that is unused and has type
`Result` triggers the `unused_result` warning (§10).

### 4.4 Command calls

A statement of the form `IDENT arg, …` (without parentheses), where `IDENT` is directly
followed by whitespace and the first argument begins with a literal, an identifier, an
`AT_IDENT`, `self`, or a keyword-argument label. It is equivalent to `IDENT(arg, …)`.
Example: `puts "hi"`.

Precisely: the statement starts with an `IDENT` that is followed by whitespace (not a line
terminator) and then by a token that can begin a command argument: a literal, the start of
a string, an `IDENT`, an `AT_IDENT`, `self` or `Self`. A label (`IDENT :`) also qualifies.
`puts -1` and `puts [1]` are therefore binary and index expressions, not command calls.

## 5. Types

*TODO(Phase 2).* Decided: primitives, built-in generics, tuples, function types `Fn`,
`FnMut`, `FnOnce`, `dyn Trait`, `ref T` / `mut ref T` (return positions only, §8.3), `Self`,
`Never`, `Unit`. Generic arguments use `[ ]`. See
[ADR 0004](../docs/adr/0004-type-system-and-inference.md).

### 5.1 Primitive types
### 5.2 Built-in generic types
### 5.3 Tuples
### 5.4 Function and closure types
### 5.5 Trait objects
### 5.6 Reference types (`ref T`)

## 6. Type checking and inference

*TODO(Phase 2).* Annotations are required at item boundaries. Bodies are inferred
bidirectionally, within one function at a time. Integer literals default to `Int`, float
literals to `Float`. Uniform access resolves `x.name` to a field, then to a zero-argument method.

## 7. Expressions

*TODO(Phase 1/2):* full semantics. The syntax is in `grammar.ebnf`.

### 7.1 Literal expressions
### 7.2 Path expressions

Paths use `.` only. Whether `A.b` is a module member, an associated item, a field or a
method is decided by name resolution, not by syntax. Likewise, `Name[Args]` in
expression position is a generic instantiation if `Name` resolves to a type, and an index
expression otherwise.

### 7.3 Calls and trailing blocks

`f(args)`, `f(args) { |x| … }`, `f(args) do |x| … end`, `f { … }`. A trailing block is
passed as the final argument. A standalone closure value is written `{ |params| body }`; the
parameter list is required, so `{` in expression position without `|` is a map literal (§7.13).

A `{ … }` or `do … end` block is a trailing block of the expression before it only if it
starts on the same line as the end of that expression. A trailing block's parameter list is
optional: `s.spawn { work() }`.

### 7.4 Field access, method calls and `@field`

`expr.name`, `expr.name(args)`, `expr.0` (tuple). Inside a method, `@name` means `self.name`.

### 7.5 Operators and precedence

From highest to lowest:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `.` field/method, call `()`, index `[]`, `?` | left |
| 2 | unary `-`, `~` | prefix |
| 3 | `*` `/` `%` | left |
| 4 | `+` `-` | left |
| 5 | `<<` `>>` | left |
| 6 | `&` | left |
| 7 | `^` | left |
| 8 | `\|` | left |
| 9 | `..` `..=` | none |
| 10 | `==` `!=` `<` `<=` `>` `>=` `in` (pattern test) | none (chaining `a < b < c` is an error) |
| 11 | `not` | prefix |
| 12 | `and` | left |
| 13 | `or` | left |
| 14 | `=` and compound assignment | right; statement-level only |

`not` binds *looser* than comparison, as in Python and English: `not a == b` means
`not (a == b)`, and `not a and b` means `(not a) and b`. This differs from Rust's `!`,
which binds tightly; Loom has no `!`, so there is only one reading to learn.

### 7.6 `if` expressions

Includes the pattern test `if expr in Pattern` (§9).

### 7.7 `case` expressions
### 7.8 Loops: `loop`, `while`, `for`, `break`, `next`
### 7.9 Closures

A block parameter is a single pattern atom (`block_param` in the grammar), so a top-level
or-pattern must be parenthesized: `{ |(A | B)| … }`. This lets the parser treat the next
`|` as the end of the parameter list.
### 7.10 Ranges
### 7.11 The `?` operator — see §10.3
### 7.12 String interpolation
### 7.13 Array, map and tuple literals

`[a, b]` builds an `Array`, `{k => v, …}` a `Map`, `(a, b)` a tuple, and `()` is the unit
value. `{}` in expression position is an empty `Map`. A `{` directly after a call is a block.

### 7.14 `return`

## 8. Ownership and borrowing

*TODO(Phase 3):* normative rules. Summary of
[ADR 0005](../docs/adr/0005-ownership-and-borrowing.md):

### 8.1 Ownership, moves, `Copy` and `Drop`
### 8.2 Parameter modes: borrow (default), `mut`, `own`
### 8.3 Second-class references, elided borrow returns, and view types
### 8.4 Closure captures and escaping
### 8.5 Shared ownership (`Shared[T]`)
### 8.6 Drop order

## 9. Patterns

*TODO(Phase 2):* literal, wildcard `_`, binding, tuple, enum variant (positional and
`field:` shorthand), struct, range, or-patterns `A | B`, and guards (`in P if cond`).
Exhaustiveness is checked for `case`; `x in P` is a `Bool` test that binds on success.

## 10. Error handling

*TODO(Phase 2).* Summary of [ADR 0007](../docs/adr/0007-error-handling.md):

### 10.1 `Option`
### 10.2 `Result` and the `Error` trait
### 10.3 The `?` operator
### 10.4 Panics and unwinding
### 10.5 `main` return types and exit status

## 11. Concurrency

*TODO(Phase 4).* Summary of [ADR 0006](../docs/adr/0006-concurrency-model.md):

### 11.1 Tasks and `scope`
### 11.2 Channels
### 11.3 `select`

An arm `when P = e` receives into the pattern `P`, and an arm `when e` waits on `e`. The
parser tells them apart by looking for a `=` at bracket depth 0 before `then` or the end of
the line.
### 11.4 Cancellation and deadlines
### 11.5 `Send` and `Sync`
### 11.6 Detached tasks

## 12. Modules and packages

*TODO(Phase 5).* Summary of [ADR 0010](../docs/adr/0010-modules-and-packages.md):

### 12.1 Files and modules
### 12.2 Visibility and paths
### 12.3 Packages and `loom.toml`

## 13. Unsafe code and FFI

*TODO(Phase 3/5).* `unsafe` blocks and functions have Rust's meaning. `extern "C"`
declarations. See [ADR 0016](../docs/adr/0016-rust-interop.md).

## 14. Program execution

*TODO(Phase 2):* `main`, program arguments (`Env.args`), exit status, the order of global
initialization (none: constants only), and output buffering of `puts`.

---

## Appendix A. Grammar

See [`grammar.ebnf`](grammar.ebnf).

## Appendix B. Change log

| Date | Change |
|---|---|
| 2026-10-23 | Initial draft (Phase 0): lexical structure complete; other chapters outlined. |
| 2026-10-28 | Phase 1 (ADR 0018): §2.3 rule 1(a) ignores trivia; `LINE_BREAK` trivia; disambiguation rules in §4.1, §4.4, §7.3, §7.9, §11.3; Appendix C diagnostic codes. |

## Appendix C. Diagnostic codes

Error codes and their primary positions are part of conformance
(`spec/conformance/README.md`). Message wording is not. Codes are allocated by range
([ADR 0018](../docs/adr/0018-front-end-design.md)): E0001–E0099 lexical, E0100–E0199
syntax, E0200–E0299 names and modules, E0300–E0499 types and traits, E0500–E0599
ownership, E0600–E0699 concurrency. A code is never reused. The "At" column gives the
primary position, which is where the diagnostic points.

### C.1 Lexical errors (§2)

| Code | Error | At | Rule |
|---|---|---|---|
| E0001 | Source file is not valid UTF-8 | first invalid byte | §2.1.1 |
| E0002 | Bidirectional formatting character | the character | §2.1.4 |
| E0003 | Carriage return not followed by line feed | the CR | §2.2.1 |
| E0004 | Non-ASCII whitespace character | the character | §2.2.2 |
| E0005 | Unexpected character | the character | §2 |
| E0006 | `;` is not used in Loom | the `;` | §2.3.4 |
| E0007 | `!` is not an operator | the `!` | §2.8.3 |
| E0008 | `&&` or `\|\|` is not an operator | the operator | §2.8.4 |
| E0009 | Reserved word used as an identifier | the word | §2.6 |
| E0010 | Misplaced `_` in a numeric literal | the literal | §2.7.1.1 |
| E0011 | Decimal literal with a leading zero | the literal | §2.7.1.2 |
| E0012 | Invalid or missing digits in a numeric literal | the literal | §2.7.1, §2.7.2 |
| E0013 | Uppercase radix prefix | the literal | §2.7.1.3 |
| E0014 | Invalid suffix on a numeric literal | the suffix | §2.7.1.4 |
| E0015 | Empty character literal | the literal | §2.7.3.2 |
| E0016 | Character literal with more than one character | the literal | §2.7.3.2 |
| E0017 | Unterminated character literal | the opening `'` | §2.7.3 |
| E0018 | Unterminated string literal | the opening delimiter | §2.7.4 |
| E0019 | Unknown escape sequence | the escape | §2.7.5 |
| E0020 | Invalid `\u{…}` escape | the escape | §2.7.5 |
| E0021 | Invalid `\x` escape | the escape | §2.7.5 |
| E0022 | Non-ASCII character in a byte string | the character | §2.7.4.5 |
| E0023 | Opening `"""` not followed by a line break | the `"""` | §2.7.4.3 |
| E0024 | Closing `"""` not on its own line | the `"""` | §2.7.4.3 |
| E0025 | Line in a triple-quoted string lacks the indentation prefix | start of the line | §2.7.4.3 |
| E0026 | Unterminated raw string | the opening `r` or `br` | §2.7.4.4 |
| E0027 | `#![` at the start of a file is reserved | the `#` | §2.1.3 |

### C.2 Syntax errors (§3–§11)

| Code | Error | At |
|---|---|---|
| E0100 | Expected a different token (general syntax error) | the unexpected token |
| E0101 | Unclosed delimiter `(`, `[`, `{` or `#{` | the opening delimiter |
| E0102 | Missing `end` | the keyword that opened the block |
| E0103 | Unexpected closing delimiter or `end` | the delimiter |
| E0104 | Comparison operators cannot be chained | the second operator |
| E0105 | Range operators cannot be chained | the second operator |
| E0106 | Assignment is only allowed as a statement | the `=` or compound operator |
| E0107 | Positional argument after a named argument | the positional argument |
| E0108 | Inner doc comment (`##!`) after an item | the comment |
| E0109 | `impl` without a trait; inherent methods belong in the type's body | the `impl` keyword |
| E0110 | `else if` is written `elsif` | the `if` |
| E0111 | Two statements on one line | the start of the second statement |
| E0112 | Nesting too deep | the token where the limit was reached |
| E0113 | Glob imports are not supported | the `*` |

If a token is missing at the end of a line, E0100 points at the line terminator (or at
end of file).

### C.3 Warnings

Warnings are named lints, not numbered codes.

| Lint | Warning | Rule |
|---|---|---|
| `dangling_doc_comment` | Outer doc comment not followed by an item, field, variant or method | §2.4.2 |
