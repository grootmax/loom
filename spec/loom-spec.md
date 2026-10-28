# The Loom Language Specification

- **Edition:** `preview` (pre-1.0; no compatibility guarantees yet)
- **Status:** Draft. §2–§7, §9, §10, §12.1–§12.2, §12.4 and §14 are complete enough to
  implement against; other chapters are outlines marked `TODO(Phase N)` with the rules
  already decided in ADRs.
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
   - (c) The line it ends contains no tokens (it is blank or holds only whitespace and
     comments). By rule 3 such a `NEWLINE` would be redundant.
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

An item is a declaration at module level, or inside a struct, enum, trait or `impl` body.
Items are visible throughout their module regardless of their order (there are no
forward declarations).

### 3.1 Functions

1. `def name[Generics](params) -> ReturnType … end`, or the single-expression form
   `def name(params) -> T = expr`. The parameter list may be omitted when there are no
   parameters (`def main`). A function with no `->` returns `Unit`. The value of the
   body's final expression statement is the return value; if the body ends with a
   statement that is not an expression, or the return type is `Unit`, the body's value is
   discarded and `()` is returned.
2. **Parameters** are `[mode] name: Type [= default]`. Modes are described in §8.2:
   borrow (no keyword), `mut` and `own`. Parameter names in one list must be distinct.
3. **Default values.** A parameter with a default may be omitted at a call. The default
   expression is evaluated at each call that omits it, after all given arguments, in the
   scope of the function's module, and may refer to parameters declared before it. It is
   an error for a parameter without a default to follow one with a default, except for a
   final parameter of callable type, which may always be given as a trailing block.
4. **Arguments.** At a call, positional arguments come first, then named arguments
   (`name: expr`, or the shorthand `name:` meaning `name: name`). Positional arguments
   bind to parameters in order. Each named argument binds to the parameter of that name.
   It is an error to name a parameter that does not exist, to give a parameter twice, to
   give a positional argument after a named one, or to omit a parameter that has no
   default. A trailing block (§7.3) is the last argument.
5. **Methods** are functions declared inside a struct or enum body (inherent methods), a
   trait, or an `impl`. A method has a receiver `self` of the enclosing type, with a mode
   given after `def`: `def name` borrows `self`, `def mut name` borrows it mutably, and
   `def own name` consumes it. `def self.name` declares an *associated function*, which has
   no receiver and is called through the type (`Counter.new(…)`).
6. `main` (§14) is an ordinary function with no parameters.

### 3.2 Structs

1. `struct Name[Generics] … end` declares a struct type with named fields `name: Type`
   and inherent methods. Fields are private to the module unless marked `pub` (§3.11).
   Field names must be distinct.
2. **Construction** is `Name(field: value, …)` (with generic arguments `Name[T](…)` if
   they cannot be inferred). Every field must be given exactly once, by name, in any
   order. `Name(field:)` takes the value from the binding or parameter named `field`. A
   struct with no fields is constructed with `Name()`. Constructing a struct requires
   every field to be visible at the construction site.
3. A struct value's fields are read with `value.field` (§7.4).

### 3.3 Enums

1. `enum Name[Generics] … end` declares a sum type whose values are exactly one of its
   variants. A variant is a unit variant `Variant`, a positional variant `Variant(T, …)`
   or a named variant `Variant(field: T, …)`. Variant names must be distinct.
2. Variants are referred to as `Name.Variant`. Where the expected type of an expression
   (§6.4) is the enum, or in a pattern matched against the enum, the variant may be
   written unqualified: `Circle(radius: 1.0)`.
3. **Construction:** `Name.Unit`, `Name.Positional(a, b)`, `Name.Named(f: a, g: b)`. The
   arguments of a named variant follow the rules for struct construction (§3.2 rule 2).
4. Enums may contain inherent methods after or between their variants.
5. `Option` and `Result` (§10) are ordinary enums of the core library whose variants
   `Some`, `None`, `Ok` and `Err` are in the prelude (§12.4).

### 3.4 Traits

1. `trait Name[Generics] [: Supertraits] … end` declares a trait. Its members are:
   *required methods* (a signature without a body), *provided methods*
   (`default def m … end`, a method with a body that implementations may replace),
   *associated types* (`type Item`, optionally with bounds) and associated constants.
   Body-less signatures appear only in traits and in `extern` blocks.
2. Inside a trait, `Self` is the implementing type and `Self.Item` names an associated
   type of the implementation.
3. A supertrait `trait B: A` means every type implementing `B` must also implement `A`,
   and `A`'s methods are available wherever `B` is known.
4. A method of a trait can be called on a value of type `T` only where `T` implements the
   trait (§6.6) and the trait is in scope: declared in or imported into the current
   module, or in the prelude.

### 3.5 Implementations

1. `impl[Generics] Trait[Args] for Type [where …] … end` implements a trait for a type.
   It must define every required method and every associated type, and may replace
   provided methods. It must not define anything the trait does not declare. Each
   method's modes, parameter types and return type must match the trait's declaration
   after substituting `Self` and the trait's generic arguments.
2. Inherent methods are written inside the type's own body (§3.2–§3.3), never in `impl`
   blocks ([ADR 0003](../docs/adr/0003-syntax-philosophy.md)).
3. **Coherence.** An `impl` must be in the package that declares the trait or the
   package that declares the type (Rust's orphan rule; the core library is a separate
   package). Two `impl`s of the same trait whose types can unify are an error.

### 3.6 Modules

Each file is a module (§12). `module name … end` declares an inline module: a child of
the enclosing module containing items.

### 3.7 Use declarations

1. `use path`, `use path as alias`, `use path.{a, b as c}` bring items into the current
   module's namespaces under their last segment's name (or the alias). No glob imports.
2. The first segment of a `use` path is resolved as in §12.2 rule 2, except that local
   bindings and generic parameters are not considered.
3. It is an error to import an item that is not visible from the importing module, or to
   import two different items under the same name in the same namespace.
4. `pub use` re-exports the imported item from the current module.

### 3.8 Constants and statics

`const NAME: Type = expr`. The expression is evaluated at compile time. In edition
`preview` it is restricted to literals, other constants, arithmetic, comparison and
logical operators on them, tuples, and struct or variant construction from such
expressions. Constants are `Copy` or are copied at each use. Associated constants
(`Float.PI`) are declared inside a type's body or a trait.

### 3.9 Type aliases

`type Name[Generics] = Type` introduces another name for a type. An alias is not a new
type: `Name` and the aliased type are interchangeable. An alias must not refer to itself.

### 3.10 Attributes

`@[name]`, `@[name(args)]` precede an item, field, variant or method. The attributes of
edition `preview`:

| Attribute | On | Meaning |
|---|---|---|
| `derive(T, …)` | struct, enum | Implement `Copy`, `Clone`, `Eq`, `Ord`, `Hash` or `Default` (§6.7) |
| `test` | function | A test function, run by `loom test` (§14.4) |
| `inline` | function | Optimization hint; no semantic effect |
| `deprecated(…)`, `stable(…)`, `unstable(…)` | any item | API lifecycle; checked by lints |
| `lang("name")` | core library only | Marks a built-in type or trait for the compiler |

An unknown attribute is an error.

### 3.11 Visibility

An item, field or method is private to its module by default: it is visible in the module
that declares it and in that module's descendants. `pub` makes it visible wherever its
parent module is visible. `pub(package)` limits it to the current package. Using an item
that is not visible is an error.

## 4. Statements

A block is a sequence of statements separated by `NEWLINE`s (§2.3). Each statement is
executed in order. The value of a block is the value of its last statement if that is an
expression statement, and `()` otherwise.

### 4.1 Bindings

`x = expr` introduces an immutable binding when `x` is not already bound in an enclosing
scope of the same function. `var x = expr` introduces a reassignable binding. A type may
be given: `x: T = expr`, `var x: T = expr`.

Syntactically, `x = expr` with a lone identifier on the left is a binding statement.
Name resolution reclassifies it as an assignment (§4.2) when `x` is a `var` binding that is
already in scope.

1. A binding's scope starts after its statement and ends at the end of the enclosing block.
2. If `x` is already bound to an immutable binding in scope, `x = expr` is an error
   (assignment to an immutable binding). `var x = …` when `x` is already bound in scope is
   an error: bindings are never shadowed within a function.
3. Without a type, the binding's type is the type of `expr`. With `x: T`, `expr` is
   checked against `T` (§6.4).

### 4.2 Assignment

`place = expr` and compound assignment, where `place` is a `var` binding, or a field or
index reached through a `var` binding or a `mut` parameter. Assigning to an immutable
binding that is in scope is an error. Bindings are never implicitly shadowed
([ADR 0003](../docs/adr/0003-syntax-philosophy.md)).

1. A place is *mutable* if it is a `var` binding, a `mut` parameter, `self` in a
   `def mut` method, a `mut` closure parameter, or a field or index of a mutable place.
   Every other place is immutable. Assigning to an immutable place, or calling a
   `def mut` method or passing a `mut` argument (§8.2) whose place is immutable, is an error.
2. `place op= expr` is `place = place op expr`, except that `place` is evaluated once.
   Both operands must have the type the operator requires (§7.5).
3. `index` places use the `[]` operator of the container (§7.4 rule 5).
4. The value of an assignment is `()`. Assignment is a statement, not an expression.

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
a string, an `IDENT`, an `AT_IDENT`, `self`, `Self` or `not`. A label (`IDENT :`) also
qualifies.
`puts -1` and `puts [1]` are therefore binary and index expressions, not command calls.

## 5. Types

Every expression has exactly one type, known at compile time. Generic arguments are
written in square brackets. See [ADR 0004](../docs/adr/0004-type-system-and-inference.md).

### 5.1 Primitive types

| Type | Values |
|---|---|
| `I8` `I16` `I32` `I64` `I128` | Two's-complement signed integers of that many bits |
| `U8` `U16` `U32` `U64` `U128` | Unsigned integers of that many bits |
| `ISize` `USize` | Integers of the target's pointer width |
| `Int` | The same type as `I64` |
| `F32` `F64` | IEEE 754 binary32 and binary64 |
| `Float` | The same type as `F64` |
| `Bool` | `true`, `false` |
| `Char` | A Unicode scalar value |
| `String` | An owned, growable UTF-8 string |
| `Bytes` | An owned, growable byte sequence |
| `Unit` | The single value `()` |
| `Never` | No values; the type of expressions that never complete (§6.3) |

Integer arithmetic that overflows panics (§7.5). The primitive types are `Copy`
except `String` and `Bytes`.

### 5.2 Built-in generic types

The core library defines `Array[T]` (a growable sequence), `Map[K, V]` (a hash map that
iterates in insertion order), `Set[T]` (a hash set that iterates in insertion order),
`Option[T]`, `Result[T, E]` (§10), `Range[T]` (§7.10), `Ordering`, and, in later phases,
`Chan[T]`, `Shared[T]` and `Mutex[T]`. They are ordinary generic types whose methods are
documented in the core library reference. A type with generic parameters must be given
the right number of generic arguments wherever it is written, unless they are inferred
(§6).

### 5.3 Tuples

`(A, B, …)` is a tuple type with two or more elements; `()` is `Unit`. A tuple value is
written `(a, b)` and its elements are read with `t.0`, `t.1`, … . A tuple is `Copy`,
`Eq`, `Ord`, `Hash` and `Clone` if all its elements are.

### 5.4 Function and closure types

`Fn(A, B) -> R`, `FnMut(A, B) -> R` and `FnOnce(A, B) -> R` are types. A value of one of
these types is a closure (§7.9) or a function used as a value. `Fn` values may be called
any number of times and do not mutate captured state; `FnMut` values may mutate captured
state; `FnOnce` values may be called once. A value of type `Fn(…) -> R` may be used where
`FnMut(…) -> R` is expected, and an `FnMut` where an `FnOnce` is expected. `-> R` may be
omitted when `R` is `Unit`.

### 5.5 Trait objects

`dyn Trait` is the type of values of any type implementing `Trait`, with methods
dispatched at run time. A value of type `T` where `T: Trait` is converted to `dyn Trait`
where a `dyn Trait` is expected (§6.5). `Trait` must be *object-safe*: none of its methods
is generic or an associated function, and `Self` appears in no method's parameter or
return type except as the receiver. Using `dyn` with a trait that is not object-safe is
an error.

### 5.6 Reference types (`ref T`)

`ref T` and `mut ref T` may appear only in a function's return type, including inside a
generic argument of it (`Option[ref V]`). They denote a borrow of a value reachable from
the function's borrowed parameters (§8.3). For type checking, `ref T` behaves as `T`.

## 6. Type checking and inference

### 6.1 Annotations

Types are required on function parameters, on return types other than `Unit`, on struct
fields, on variant payloads, and on constants. Inside a function body, types are inferred
(§6.2). Inference never uses information from another function's body.

### 6.2 Inference

1. A body is checked in order, statement by statement. Each expression is either
   *checked against an expected type* (when the context determines one, §6.4) or has its
   type *inferred* from its parts. Unknown types are represented by type variables solved
   by unification within the body.
2. An integer literal whose type is not determined by its context has type `Int`; a float
   literal, `Float`. It is an error for an integer literal not to fit its type, and for a
   float literal to be given an integer type.
3. It is an error if, at the end of a body, the type of any binding or expression is
   still not determined ("type annotations needed"). It is also an error to call a method
   or access a field on a value whose type is not yet determined at that point.
4. In a call, arguments that are not closures are checked before closures, so a closure's
   parameter types can be taken from the callee's signature.

### 6.3 `Never`

`return`, `break`, `next`, a call of a function returning `Never` (such as `panic`), and
a loop without `break` have type `Never`. An expression of type `Never` may be used where
any type is expected.

### 6.4 Expected types

The expected type of an expression comes from: a binding's annotation; a parameter's
type (for an argument); the function's return type (for the final expression of its body
and for `return`); a field's or variant payload's type (in a construction); the element
type of an array or map literal once it is known; the other branches of an `if` or `case`
whose type is known. An expression checked against an expected type must have that type,
after the coercions of §6.5.

### 6.5 Coercions

At a site with an expected type, a value is converted in exactly these cases: `Never` to
any type; `Fn` to `FnMut` to `FnOnce` (§5.4); a function to a callable type with the same
signature; and a value of type `T` to `dyn Trait` where `T: Trait`. There are no other
implicit conversions; in particular no numeric conversions.

### 6.6 Trait bounds and implementations

`T: Trait` holds if `T` is a generic parameter with that bound (directly, through a
supertrait, or through a `where` clause), or if there is exactly one `impl` whose type
unifies with `T` and whose own bounds hold. Calling a generic function or using a generic
type requires the bounds of its parameters to hold for the arguments; inside the generic
function, only the methods given by the bounds can be called on values of a parameter's
type.

### 6.7 Derived traits

`@[derive(…)]` on a struct or enum implements: `Clone` (field-wise clone); `Copy` (all
fields must be `Copy`); `Eq` (field-wise, and variants must be equal); `Ord` (by variant
declaration order, then field by field in declaration order); `Hash`; and `Default` (for
structs, each field's default). Deriving a trait requires every field type to implement it.

## 7. Expressions

The syntax is in `grammar.ebnf`. Operands are evaluated left to right, and arguments are
evaluated in the order they are written, before the call.

### 7.1 Literal expressions

Literals (§2.7) have the types `Int`-family (§6.2), `Float`-family, `Char`, `String`
(interpolated, triple-quoted and raw strings), `Bytes` (byte strings) and `Bool`.

### 7.2 Path expressions

Paths use `.` only. Whether `A.b` is a module member, an associated item, a field or a
method is decided by name resolution, not by syntax. Likewise, `Name[Args]` in
expression position is a generic instantiation if `Name` resolves to a type, and an index
expression otherwise.

A path is resolved from the left (§12.2): while the prefix names a module, the next
segment names an item of it; once the prefix names a type, the next segment names an
associated function, associated constant or variant; once it names a value, the
remaining segments are field accesses and method calls (§7.4).

### 7.3 Calls and trailing blocks

`f(args)`, `f(args) { |x| … }`, `f(args) do |x| … end`, `f { … }`. A trailing block is
passed as the final argument. A standalone closure value is written `{ |params| body }`; the
parameter list is required, so `{` in expression position without `|` is a map literal (§7.13).

A `{ … }` or `do … end` block is a trailing block of the expression before it only if it
starts on the same line as the end of that expression. A trailing block's parameter list is
optional: `s.spawn { work() }`.

The callee may be a function, an associated function, a struct or variant constructor
(§3.2–§3.3), or a value of a callable type (§5.4). Arguments bind to parameters as in
§3.1 rule 4. Calling a value that is none of these is an error.

### 7.4 Field access, method calls and `@field`

`expr.name`, `expr.name(args)`, `expr.0` (tuple). Inside a method, `@name` means `self.name`.

1. **Uniform access.** `expr.name` without arguments is a field access if the type of
   `expr` has a field `name` visible here; otherwise it is a method call with no
   arguments, and every parameter of that method must have a default.
2. `expr.name(args)`, and `expr.name` followed by a trailing block, are method calls.
   A callable stored in a field is called as `(expr.name)(args)`.
3. **Method lookup** for a receiver of type `R`: first the inherent methods of `R`; then
   the methods of traits in scope (§3.4 rule 4) that `R` implements, or that bound `R` if
   it is a generic parameter; then, for `dyn Trait`, the methods of `Trait`. It is an
   error if no method is found, or if two traits in scope provide a method of that name
   for `R` and no inherent method does.
4. The receiver is passed in the method's receiver mode (§8.2); a `def mut` method
   requires a mutable place (§4.2 rule 1).
5. **Indexing** `expr[i]` reads an element: for `Array[T]`, `i` is an `Int`, and an index
   outside `0 ≤ i < length` panics; for `Map[K, V]`, `i` is a `K`, and a missing key
   panics. `expr[i] = v` writes an `Array` element or inserts or replaces a `Map` entry.

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

1. **Arithmetic** `+ - * / %` and unary `-` apply to two operands of the same numeric
   type (unary `-`: signed integers and floats) and produce that type. Integer `+ - *`
   and unary `-` panic on overflow. Integer `/` and `%` truncate toward zero and panic if
   the divisor is zero or the result overflows (`I64.MIN / -1`). Float arithmetic follows
   IEEE 754 with round-to-nearest-even.
2. **Bitwise** `& | ^` apply to two operands of the same integer type, or to two `Bool`s
   (without short-circuit); `~` to an integer. **Shifts** `<<` `>>` take two operands of
   the same integer type; `>>` is arithmetic for signed types. A shift amount that is
   negative or not less than the type's bit width panics.
3. **Equality** `==` `!=` require both operands to have the same type `T` with `T: Eq`.
   **Ordering** `< <= > >=` require `T: Ord`. For floats, these follow IEEE 754 (any
   comparison with NaN is `false`, except `!=`).
4. **Logic** `and` `or` take `Bool` operands and short-circuit; `not` takes a `Bool`.
5. `expr in pattern` is the pattern test (§9.3).

### 7.6 `if` expressions

Includes the pattern test `if expr in Pattern` (§9).

1. `if c … elsif d … else … end` evaluates the conditions in order, which must have type
   `Bool`, and evaluates the branch of the first that is `true`, or the `else` branch.
2. With an `else`, the branches must have a common type, which is the type of the `if`.
   Without one, the type is `Unit` and each branch's value is discarded.
3. A condition that is a pattern test, or an `and` chain containing pattern tests, makes
   the tests' bindings available in the rest of the chain and in that branch (§9.3).

### 7.7 `case` expressions

`case scrutinee` followed by arms `in pattern [if guard] then expr` or
`in pattern [if guard]` with a block, and optionally a final `else`. The scrutinee is
evaluated once; the arms are tried in order; the first whose pattern matches and whose
guard (if any) evaluates to `true` is evaluated with the pattern's bindings, and its
value is the value of the `case`. The arms must have a common type. The arms together
with `else` must be exhaustive (§9.4).

### 7.8 Loops: `loop`, `while`, `for`, `break`, `next`

1. `loop do … end` repeats its body until `break`. Its type is the type of the values
   given to its `break`s, or `Never` if it has none.
2. `while c … end` evaluates `c` (a `Bool`, or a pattern-test chain, §9.3) before each
   iteration and stops when it is `false`. Its type is `Unit`.
3. `for pattern in expr … end` iterates: if the type of `expr` implements `Iterator`, its
   values are taken by calling `next` until it returns `None`; otherwise the type must
   implement `Iterable`, and the loop iterates `expr.iter`. Each value is matched against
   the pattern, which must be irrefutable. Its type is `Unit`.
4. `break` leaves the innermost loop; `break value` (only in `loop`) gives the loop's
   value. `next` continues with the next iteration. Both are errors outside a loop, and
   they do not cross a closure boundary.

### 7.9 Closures

A block parameter is a single pattern atom (`block_param` in the grammar), so a top-level
or-pattern must be parenthesized: `{ |(A | B)| … }`. This lets the parser treat the next
`|` as the end of the parameter list.

1. A closure `{ |params| body }` or `do |params| … end` is a value of a callable type
   (§5.4). Parameter types may be annotated (`|x: Int|`); otherwise they come from the
   expected type, and it is an error if they cannot be determined.
2. A trailing block whose callee expects a callable with no parameters may omit `||`.
   A trailing block whose callee expects one parameter but whose block has none is an error.
3. A closure may use the bindings of its enclosing function (captures), and may assign
   to captured `var` bindings. A closure that assigns to a capture or passes it in `mut`
   mode is an `FnMut`; otherwise it is an `Fn`.
4. `return` inside a closure returns from the closure. `?` propagates out of the closure.

### 7.10 Ranges

`a..b` (half-open) and `a..=b` (inclusive) build a `Range[T]` where `a` and `b` have the
same integer or `Char` type `T`. Ranges implement `Iterator` and yield values in
increasing order; an empty range yields nothing.

### 7.11 The `?` operator — see §10.3

### 7.12 String interpolation

1. `"…#{expr}…"` evaluates each embedded expression left to right and concatenates the
   literal fragments and the expressions' `Display` representations. Each embedded
   expression's type must implement `Display`. The result is a new `String`.
2. The `Display` representations of the primitive types are: integers in decimal, with a
   leading `-` if negative; `true` and `false`; a `Char` or `String` as its own text; `()`
   for `Unit`. A float is written as the shortest decimal that converts back to the same
   value; if it is zero or `1e-4 ≤ |x| < 1e16`, it is written positionally with at least
   one digit after the point (`7.0`, `0.25`, `-0.0`), and otherwise as a mantissa and an
   exponent (`1e16`, `1.5e-7`). Infinities are `inf` and `-inf`, and NaN is `NaN`.

### 7.13 Array, map and tuple literals

`[a, b]` builds an `Array`, `{k => v, …}` a `Map`, `(a, b)` a tuple, and `()` is the unit
value. `{}` in expression position is an empty `Map`. A `{` directly after a call is a block.

All elements of an array literal must have one type; all keys of a map literal one type,
and all values one type. Map keys must implement `Eq` and `Hash`. A map literal with a
repeated key keeps the last value at the position of the first.

### 7.14 `return`

`return expr` evaluates `expr` and returns it from the innermost function or closure;
`return` alone returns `()`. The value is checked against the return type.

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

A pattern is matched against a value. It either matches, possibly binding names to parts
of the value, or does not.

### 9.1 Pattern forms

| Pattern | Matches |
|---|---|
| `_` | anything; binds nothing |
| `name` (lowercase first letter) | anything; binds `name` to the value |
| literal (`3`, `-1`, `'a'`, `"x"`, `true`) | a value equal to the literal |
| `a..b`, `a..=b` (integer or `Char` literals) | a value in the range |
| `(p1, p2, …)` | a tuple whose elements match the patterns |
| `Path` (uppercase first letter) | the unit variant, unit struct or constant named by the path |
| `Path(p1, p2)` | a positional variant whose payload matches, element by element |
| `Path(f: p, g:, ..)` | a named variant or struct whose fields match; `g:` is short for `g: g` |
| `p1 \| p2` | a value matching either pattern |

1. **Capitalization decides.** An identifier pattern whose first character is an uppercase
   letter is a path (a variant, a struct or a constant), never a binding. Any other
   identifier is a binding.
2. **Unqualified variants.** A path pattern with one segment that is not an item in scope
   names a variant of the enum type being matched (`in Circle(..)` against a `Shape`). It
   is an error if the matched type is not an enum with such a variant.
3. In a positional payload, `..` matches the remaining elements. In a named payload,
   fields may be given in any order, and `..` (last) matches the fields not listed;
   without `..`, every field must be listed.
4. A literal pattern's type must be the type being matched. A range pattern's bounds must
   satisfy `a < b` for `..` and `a ≤ b` for `..=`.
5. Both sides of an or-pattern must bind the same names with the same types. A name may
   be bound only once in a pattern.
6. Bindings are immutable and take the matched part of the value (a borrow of it if the
   matched value is borrowed; ownership rules are in §8).

### 9.2 Refutability

A pattern is *irrefutable* if it matches every value of its type: `_`, bindings, tuples
of irrefutable patterns, and structs or single-variant enums whose fields are
irrefutable. Patterns in `for` loops, closure parameters and bindings must be irrefutable.

### 9.3 Pattern tests

`expr in pattern` evaluates `expr` and has type `Bool`: whether the value matches. A
pattern test that binds names may appear only as the condition of an `if`, `elsif` or
`while`, or as an operand of an `and` chain that is such a condition. Its bindings are
in scope in the operands to its right and in the body that the condition guards. Anywhere
else, a pattern test must not bind names.

### 9.4 Exhaustiveness

The arms of a `case` without `else` must be exhaustive: every value of the scrutinee's type
must match some arm's pattern. Arms with a guard do not count toward exhaustiveness.
Integers and `Char` are exhaustive only through `_`, a binding, or ranges covering the
whole type; `String` and float values only through `_` or a binding. A non-exhaustive
`case` is an error; implementations should list some missing patterns. An arm that can
never match because earlier arms cover it triggers the `unreachable_pattern` warning.

## 10. Error handling

See [ADR 0007](../docs/adr/0007-error-handling.md).

### 10.1 `Option`

`enum Option[T]` has the variants `Some(T)` and `None`. It represents a value that may be
absent; there is no null.

### 10.2 `Result` and the `Error` trait

`enum Result[T, E]` has the variants `Ok(T)` and `Err(E)`. The `Error` trait has a required
method `def message -> String` and a provided method `def source -> Option[ref dyn Error]`
returning `None`. A `main` that returns `Result` requires `E: Error` (§14.2).

### 10.3 The `?` operator

`expr?` applies to a value of type `Result[T, E]` or `Option[T]`:

1. In a function or closure whose return type is `Result[U, F]`, for `expr: Result[T, E]`:
   if the value is `Ok(v)`, the result is `v`, of type `T`; if it is `Err(e)`, the function
   returns `Err(F.from(e))`. `F: From[E]` must hold; every type implements `From` of itself.
2. In a function or closure whose return type is `Option[U]`, for `expr: Option[T]`: if
   the value is `Some(v)`, the result is `v`; if it is `None`, the function returns `None`.
3. Any other use is an error.

### 10.4 Panics and unwinding

A panic stops the current task's normal execution and unwinds it (ADR 0007). In a program
without tasks, a panic that is not caught by a task scope ends the program: the
implementation writes a message that contains the panic message and its source position to
standard error and exits with status 101. The language's own panics are: integer overflow,
integer division or remainder by zero, a shift amount out of range, an index out of range
or missing key, `panic(message)`, `todo()`, a failed `assert` or `assert_eq`, and
`unwrap` of `None` or `Err`.

### 10.5 `main` return types and exit status

See §14.2.

### 10.6 Unused results

A statement whose expression has type `Result[T, E]` and whose value is not used triggers
the `unused_result` warning.

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

Summary of [ADR 0010](../docs/adr/0010-modules-and-packages.md). Packages and
`loom.toml` are *TODO(Phase 5)*.

### 12.1 Files and modules

1. A package is a tree of modules. Each source file is a module; `module name … end`
   declares a child module inline. In edition `preview`, a program run with `loom run`
   is a single root file and its inline modules (multi-file packages: Phase 5).
2. Every module has two namespaces: *types* (modules, structs, enums, traits, type
   aliases) and *values* (functions, constants, statics). A name may be declared once in
   each namespace of a module; a second declaration is an error. A struct's name is also
   its constructor in the value namespace, so a struct and a function of the same name
   in one module are an error.

### 12.2 Visibility and paths

1. Visibility is defined in §3.11.
2. The first segment of a path is resolved by looking, in order, at: the local bindings
   in scope (§4.1); the generic parameters in scope; the items declared in or imported
   into the current module; the prelude (§12.4); and the names of packages (`core`,
   `std`). The keywords `package`, `super` and `self` as a first segment name the package
   root, the parent module and the current module (inside a method body, `self` is the
   receiver instead).
3. Each further segment is resolved in the entity named by the prefix: an item of a
   module; an associated item or variant of a type; a field or method of a value (§7.2).

### 12.3 Packages and `loom.toml`

*TODO(Phase 5).*

### 12.4 The prelude

Every module implicitly sees these names from the core library, after its own items and
imports (§12.2 rule 2): the primitive types (§5.1); `Array`, `Map`, `Set`, `Option`,
`Result`, `Range`, `Ordering`; the traits `Eq`, `Ord`, `Hash`, `Clone`, `Copy`,
`Display`, `Default`, `From`, `Iterator`, `Iterable`, `Error`; the variants `Some`,
`None`, `Ok`, `Err`; and the functions `puts`, `print`, `panic`, `todo`, `assert` and
`assert_eq`.

## 13. Unsafe code and FFI

*TODO(Phase 3/5).* `unsafe` blocks and functions have Rust's meaning. `extern "C"`
declarations. See [ADR 0016](../docs/adr/0016-rust-interop.md).

## 14. Program execution

### 14.1 `main`

A program's root module must declare a function `main` with no parameters. Running the
program calls `main`. There is no other global initialization: constants are evaluated
at compile time, and there are no mutable globals.

### 14.2 Exit status

`main` returns `Unit` or `Result[Unit, E]` with `E: Error`. When `main` returns `()` or
`Ok(())`, the program exits with status 0. When it returns `Err(e)`, the implementation
writes `Error: ` followed by `e.message` and a line terminator to standard error and
exits with status 1. A panic that reaches `main` exits with status 101 (§10.4).

### 14.3 Standard output

`puts(value)` writes the `Display` representation of `value` (§7.12 rule 2) followed by
`"\n"`; `print(value)` writes it without the line terminator. Output may be buffered, but
everything written must appear, in program order, before the program exits, and before
any message the implementation writes to standard error.

### 14.4 Tests

`loom test` runs every function marked `@[test]` in the root module and its inline
modules, in declaration order. A test function has no parameters and returns `Unit`. A
test passes if it returns, and fails if it panics. The output format is not specified.

---

## Appendix A. Grammar

See [`grammar.ebnf`](grammar.ebnf).

## Appendix B. Change log

| Date | Change |
|---|---|
| 2026-10-23 | Initial draft (Phase 0): lexical structure complete; other chapters outlined. |
| 2026-10-28 | Phase 1 (ADR 0018): §2.3 rule 1(a) ignores trivia; `LINE_BREAK` trivia; disambiguation rules in §4.1, §4.4, §7.3, §7.9, §11.3; Appendix C diagnostic codes. |
| 2026-10-28 | Phase 2 (ADR 0019): normative text for §3–§7, §9, §10, §12.1–§12.2, §12.4 and §14; capitalization classifies identifier patterns (§9.1); `ref T` is checked as `T` until Phase 3 (§5.6); Display rules for primitives (§7.12). |

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
