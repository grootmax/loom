# ADR 0019: Semantic analysis and the reference interpreter (Phase 2)

- **Status:** Accepted (provisional; revisited at the end of Phase 2 together with ADR 0004)
- **Date:** 2026-10-28
- **Deciders:** lead architect
- **Related:** [ADR 0004](0004-type-system-and-inference.md), [ADR 0005](0005-ownership-and-borrowing.md),
  [ADR 0007](0007-error-handling.md), [ADR 0008](0008-compiler-architecture.md),
  [ADR 0010](0010-modules-and-packages.md), [ADR 0018](0018-front-end-design.md),
  spec §3–§7, §9, §10, §12, §14

## Context

Phase 2 turns parsed Loom into checked, runnable Loom: name resolution, HIR, type
inference and checking, and a reference interpreter. Its exit criterion is that
`loom run hello.loom` works and that the conformance suite covers expressions, functions,
structs, enums, traits, generics, pattern matching, `Option` and `Result`. The 18 examples
marked `# phase: 2` must produce their `.stdout` byte for byte.

ADRs 0004, 0005 and 0007 fix the language-level rules. This ADR fixes everything those
leave open that an implementation has to decide: the crates and their data, the rules
of name resolution, the inference algorithm, how built-in types and the core library are
defined, and what the interpreter's value model is. Each decision that changes what a
program means is also written into the spec.

## Decision

### 1. Crates and data flow

```
loom_ast ─► loom_hir (item tree + body lowering, syntactic)
         ─► loom_resolve (module tree, imports, visibility, scopes → resolved paths)
         ─► loom_types (Ty, interning, substitution, unification table)
         ─► loom_check (signatures, trait/impl tables, inference, exhaustiveness → TypedBody)
         ─► loom_interp (evaluates HIR + TypedBody)
```

- **`loom_hir`** owns the IDs and the arena-based IR. `ItemTree`: per module, every item
  with its name, visibility, attributes, generic parameters and *unresolved* signature
  (`TypeRef`, `Path`). `Body`: per function, constant or default-argument expression, an
  arena of `Expr`, `Pat` and `Stmt` with `ExprId`/`PatId` indices and a source map back to
  CST ranges. Lowering is purely syntactic and total: it never fails, and a node the
  parser could not build lowers to `Expr::Missing`. Desugarings done here: `@x` → field
  access on `self`; command calls → calls; trailing blocks → last argument; compound
  assignment kept as one node (so diagnostics point at the operator); `elsif` → nested
  `if`; string interpolation → `Expr::Interpolate(parts)`.
- **`loom_resolve`** builds the `DefMap` (module tree, the two namespaces of each module,
  imports, visibility), then resolves every path in signatures and bodies. Its output is a
  side table, `ExprId`/`PatId`/`TypeRefId` → `Res` (`Local(BindingId)`, `Def(DefId)`,
  `PrimTy`, `SelfTy`, `TyParam`, `Partial { res, unresolved_segments }`, `Unresolved`).
  It also decides, per `BIND_STMT`, whether it is a binding or an assignment (spec §4.1).
- **`loom_types`** defines `Ty` (interned, `Copy`, cheap to compare), generic substitutions
  and the unification table. No knowledge of HIR.
- **`loom_check`** lowers signatures to `Ty`, builds the trait and impl tables, checks
  coherence and well-formedness, infers every body and checks exhaustiveness. Output per
  body: `TypedBody { expr_ty, pat_ty, method_res, field_res, coercions, … }`.
- **`loom_interp`** walks HIR using `TypedBody`. It never re-derives a type or a resolution.
- **`loom_driver`** owns a `Program` (all source files plus the embedded core library) and
  exposes query-shaped methods (`def_map()`, `body(def)`, `typed_body(def)`, …) memoized
  with `OnceCell`. **No `salsa` in Phase 2.** The API is shaped so that salsa (or our own
  engine) can replace the memoization when the language server needs incrementality
  (Phase 6). This amends ADR 0008 §5 and ADR 0018 §11, and keeps the workspace at zero
  dependencies.

IDs: `DefId` (a global index into the item tree arena: module, struct, enum, variant,
field, trait, impl, function, const, type alias, type parameter, associated type),
`BindingId` and `ExprId`/`PatId` (per body). All IDs are `u32` newtypes.

### 2. Name resolution (spec §3.6–§3.7, §4.1, §7.2, §12)

1. **Two namespaces per module**, as in Rust: *types* (modules, structs, enums, traits,
   type aliases, packages) and *values* (functions, constants, statics). A name may exist
   once in each namespace. A duplicate in one namespace is an error.
2. **Lookup order** for the first segment of a path: local bindings (innermost first) →
   generic parameters → items of the current module (including its `use` imports) →
   the **prelude** → package names (`core`, `std`). `package`, `super` and `self` (in a
   path's first position, outside a method body) name the package root, the parent module
   and the current module. Items shadow the prelude; nothing shadows a local binding,
   because bindings are never shadowed (§4.1).
3. **Visibility** is Rust's: a private item is visible in its own module and in that
   module's descendants. `pub(package)` is visible in the package. `pub` is visible
   everywhere the containing module is. Fields and methods carry their own visibility.
4. **Imports** (`use`) resolve their first segment as in rule 2, minus locals. Imports are
   resolved to a fixed point so that their order in the file does not matter, and cycles
   between modules are allowed (ADR 0010). There are no glob imports. A `use` of a
   private item is an error at the `use`.
5. **Dotted paths in expressions.** `a.b.c` is resolved from the left. As long as the
   prefix names a module, the next segment is looked up in it. Once the prefix names a
   type (or a type with generic arguments, `Stack[String]`), the rest are associated items
   or variants, resolved by `loom_check`. Once it names a value, the rest are fields and
   methods, also for `loom_check`. Resolution therefore records a `Partial` result and the
   checker finishes it.
6. **Patterns.** An identifier pattern whose first character is an uppercase letter is
   never a binding: it is a path to a variant, unit struct or constant. If no such item is
   in scope, it names a variant of the scrutinee's enum type, which the checker resolves
   (`in Circle(..)` when matching a `Shape`). Any other identifier pattern is a binding.
   This classification needs no type information and is the same everywhere.
7. **Unqualified variants in expressions** (`Circle(radius: 1.0)` where a `Shape` is
   expected) are resolved by the checker against the expected type; resolution records
   `Unresolved` for them without an error, and the checker reports E0201 if the expected
   type does not settle it.
8. **Binding versus assignment** (§4.1). `x = e`, with a lone identifier on the left:
   if `x` is a `var` binding in scope, it is an assignment; if `x` is an immutable binding
   in scope, it is error E0203 (assignment to an immutable binding), with a fix-it that
   adds `var`; otherwise it introduces a new binding whose scope runs to the end of the
   enclosing block. Closures see the bindings of their enclosing function.
9. **Scopes.** Every block (function body, `then`/`else` branch, loop body, `case` arm,
   closure body) is a scope. Bindings introduced by an `in` pattern used as an `if` or
   `while` condition, or as an operand of an `and` chain in such a condition, are in scope
   in the following operands of the chain and in the body. A pattern that binds is
   rejected anywhere else (E0204), since its bindings would be unusable.
10. **Programs in Phase 2** are one root file plus its inline `module`s plus the core
    library. Multi-file packages (directory = module tree, `loom.toml`) arrive with
    packages in Phase 5; the `DefMap` already supports a tree of file modules.

### 3. Types (spec §5)

1. `Ty` has these kinds: the primitives of ADR 0004 (`Int` is the same type as `I64`, and
   `Float` the same as `F64`), `Unit`, `Never`, tuples, `Adt(DefId, args)` for structs and
   enums (including the built-in generic types, which are core-library structs, §4),
   `Param(DefId)` for generic parameters, `Projection(trait_ref, assoc)` for associated
   types not yet normalized, `Callable(kind, params, ret)` for `Fn`/`FnMut`/`FnOnce`,
   `Dyn(trait_ref)`, inference variables (general, integer, float) and `Error` (which
   unifies with everything and suppresses cascades).
2. **`ref T` and `mut ref T` are checked as `T` in Phase 2.** They are accepted only in
   return position (anywhere else is E0302) and recorded in the signature, so that the
   Phase 3 borrow checker can enforce ADR 0005. Phase 2 does not check borrows, moves or
   escapes; those errors (E0500–E0599) come with Phase 3.
3. **Function and closure types.** `Fn(A, B) -> R`, `FnMut(…) -> R` and `FnOnce(…) -> R`
   are types, not only bounds. A value of such a type is any function item or closure with
   that signature; the compiler decides the representation (as with `dyn`, example 13).
   A closure is `Fn` unless its body assigns to a captured binding or passes it in `mut`
   mode, in which case it is `FnMut`. `Fn` coerces to `FnMut`, and `FnMut` to `FnOnce`.
   Detecting closures that must be `FnOnce` (they move a capture out) needs move analysis
   and arrives with Phase 3.
4. **`dyn Trait`** is a type. A value of type `T` coerces to `dyn Trait` where `T:
   Trait` and a `dyn Trait` is expected (§6.5). The trait must be object-safe (Rust's rules,
   simplified: no generic methods, no `Self` in argument or return types except as the
   receiver, no associated functions without a receiver). Not object-safe is E0330.
5. **Integer literals** get an integer inference variable, which defaults to `Int`, and
   float literals a float inference variable, which defaults to `Float`. A literal that
   does not fit its final type is E0310.
6. **No implicit conversions** between numeric types (ADR 0004 rule 10). The only
   coercions are: `Never` to any type; `Fn` → `FnMut` → `FnOnce`; a function item to a
   callable type; `T` to `dyn Trait`. Coercions happen only at *coercion sites*: a binding
   with a type annotation, an argument, a returned value, a field initializer, an array
   element whose type is already known, and a branch of `if`/`case` whose expected type is
   known.

### 4. Built-in types and the core library

1. Every built-in type except the primitives, tuples, callables and `dyn` is an ordinary
   generic struct or enum written in Loom in the **core library** (`library/core/*.loom`),
   marked with `@[lang("name")]` so the compiler can find it: `Option`, `Result`, `Array`,
   `Map`, `Set`, `String`, `Range`, `Ordering`, and the traits `Eq`, `Ord`, `Hash`, `Clone`,
   `Copy`, `Display`, `Iterator`, `Iterable`, `From`, `Default`, `Error`. The primitives get
   their methods the same way: `@[lang("Int")] struct I64 … end` declares the methods of
   `I64` (the struct has no fields and cannot be constructed).
2. Operations the language cannot express are **intrinsics**, declared in
   `extern "intrinsic" … end` blocks (the existing `extern_block` grammar) and allowed only
   in `core`. Core methods call them: `def length -> Int = array_length(self)`. The
   interpreter implements each intrinsic natively; the Phase 3 backends implement the same
   list. Adding an intrinsic needs a line in `docs/internals/intrinsics.md`.
3. The core library is embedded in the compiler binary (`include_str!`) and loaded as
   package `core`. The **prelude** is a fixed list of core names that every module sees
   without importing them: the types and traits above, `Some`, `None`, `Ok`, `Err`,
   `puts`, `print`, `panic`, `assert`, `assert_eq`, `todo`. The list lives in
   `library/core/prelude.loom` as `pub use` declarations.
4. The core library's public API is part of the language and is named by the lead
   architect (CLAUDE.md "never delegate: public API naming"). Bodies of core functions
   whose signatures are written may be delegated.

### 5. Traits, methods and operators (spec §3.4–§3.5, §7.4–§7.5)

1. **Method lookup** for `recv.name(…)`: let `R` be the receiver's type (with `ref`
   ignored, §3 rule 2). Look for (a) an inherent method of `R` (declared in `R`'s body);
   then (b) a method of a trait that `R` implements and that is in scope (declared or
   imported in the current module, or in the prelude), including traits known from the
   bounds of a generic parameter; then (c) for `dyn Trait`, the trait's methods. Two
   candidates at the same step is E0322 (ambiguous). Inherent methods win over trait
   methods, as in Rust.
2. **Uniform access** (ADR 0004 rule 11). `recv.name` without an argument list is a field
   if `R` has a visible field `name`; otherwise it is a call of the method `name` with no
   arguments (all of its parameters must have defaults). `recv.name(…)` and
   `recv.name { … }` are always method calls; a callable stored in a field is called as
   `(recv.name)(…)`.
3. **Associated items** are reached through a type path: `Counter.new(…)`, `Int.parse(s)`,
   `Float.PI`, `Stack[String].new`, `Shape.Circle(radius: 1.0)`.
4. **Arguments.** Positional arguments come first, then named ones; each parameter is
   given once; a missing parameter must have a default. **Default values are evaluated at
   each call**, after the arguments, in the callee's scope, and may refer to earlier
   parameters (as in Kotlin). A trailing block is the last parameter.
5. **Operators.** Arithmetic (`+ - * / %`), bitwise (`& | ^ ~ << >>`) and unary `-` are
   defined on the numeric primitives only, with both operands of the same type (shift
   counts too). `==`/`!=` require `T: Eq` with both sides of type `T`; `< <= > >=` require
   `T: Ord`. `and`, `or`, `not` take `Bool`. `in` is the pattern test (§9). User-defined
   arithmetic operators are deferred to an RFC; they are not part of Phase 2.
   For `Float`, `==` and `<` follow IEEE 754 and `Ord.compare` uses IEEE 754 *totalOrder*,
   so sorting and `max` are total.
6. **Derives** (`@[derive(…)]`) are built into the checker for `Copy`, `Clone`, `Eq`,
   `Ord` (by variant order, then lexicographic by field order), `Hash` and `Default`.
   `Display` is not derivable. Deriving `Copy` requires every field to be `Copy` (E0340).
7. **Coherence** is Rust's orphan rule, with core treated as a foreign package. Overlapping
   impls are E0341; Phase 2 has no specialization.

### 6. Inference (spec §6)

1. Bodies are checked **bidirectionally** (`check(expr, expected)` / `infer(expr)`) with a
   unification table, one function body at a time. A closure is checked as part of the
   body that contains it.
2. **Arguments are checked in two passes**, as in rustc: first every argument that is
   not a closure, then the closures, so that a closure's parameter types are known from the
   callee's signature after the other arguments fix the generic parameters.
3. A method call on a receiver whose type is still an unresolved inference variable is
   E0282 ("type annotations needed") at the receiver, never a guess.
4. **Trait obligations** (`T: Ord`, `I: Iterator`) are collected and solved at the end
   of the body and whenever a method lookup needs them. An obligation on a concrete type
   is solved by finding the unique impl; on a generic parameter, from its bounds.
   Associated types are normalized whenever their self type is known.
5. **Defaults.** At the end of the body, unconstrained integer variables become `Int` and
   float variables `Float`. Any other unresolved variable is E0282.
6. `?` (spec §10.3): on `Result[T, E]` in a body returning `Result[U, F]`, the expression
   has type `T`, and `F: From[E]` must hold (`From[T]` is implemented for `T` itself). On
   `Option[T]` in a body returning `Option[U]`, the type is `T`. Anything else is E0360.
   In a closure, "the body" is the closure.

### 7. Patterns and exhaustiveness (spec §9)

Exhaustiveness and reachability use the usefulness algorithm of Maranget, *Warnings for
pattern matching* (2007), the algorithm rustc uses. Integers and `Char` use range
splitting; `String` literal patterns are never exhaustive without a wildcard. A `case`
that is not exhaustive is E0400, and the message lists up to three missing patterns. An
unreachable arm is the warning `unreachable_pattern`. Patterns in `for`, closure
parameters and bindings must be irrefutable (E0401).

### 8. The reference interpreter

1. **Input:** HIR plus `TypedBody`. It is a tree-walking evaluator over HIR in Phase 2 and
   moves to MIR in Phase 3, when it becomes the oracle for differential testing (ADR 0008).
2. **Values.** Scalars are stored inline (`Int` values carry their integer type, so
   overflow is checked at the right width). Strings, arrays, maps, sets, structs and enums
   with payloads are reference-counted with copy-on-write. Copying the pointer is how
   moves and borrows are implemented, and `clone` copies on the next write. Because a
   valid program never observes two owners or a mutable alias (ADR 0005), this is
   indistinguishable from Rust's value semantics, and moves are O(1).
3. **Places.** Every local is a cell. A `mut` argument passes a *place* (a cell plus a
   projection path of fields and indices), so a callee's writes are seen by the caller,
   including through `mut` closure parameters.
4. **Generics** are evaluated with the frame's type substitution, so that a trait method
   called on a `T` dispatches to the impl of the concrete type (and associated functions
   like `T.default()` work). `dyn` values carry their concrete type.
5. **Panics** (ADR 0007): integer overflow, division by zero, index out of bounds,
   `unwrap` of `None`/`Err`, `panic(…)`, `assert` failures. A panic unwinds to `main`,
   prints `panicked at <file>:<line>:<col>: <message>` to stderr and exits with status
   101. `main` returning `Err(e)` prints `Error: <e.message>` and exits with status 1.
6. **Drop.** User-defined `Drop` and destructor order (spec §8.6) are Phase 3, together
   with move checking. In Phase 2 nothing user-visible happens at scope exit.
7. Deep recursion is limited (a call depth of 10 000 frames by default) and reports a
   panic "stack overflow" instead of crashing the interpreter. The interpreter runs on a
   thread with a large stack.
8. `puts` and `print` write to a buffered stdout, flushed at exit and before anything is
   written to stderr, so output order on a terminal is the program order.

### 9. Commands

`loom check <paths>` runs every phase implemented (now through type checking).
`loom run <file> [-- args]` checks and interprets. `loom test <file>` runs `@[test]`
functions in declaration order and prints the format of example 27. `--dump=hir` and
`--dump=types` print the HIR and the inferred types (unstable formats, snapshot-tested).

### 10. Diagnostic codes

E0200–E0299 for names, modules and visibility; E0300–E0499 for types, traits, patterns
and control flow. Each code is added to spec Appendix C and the registry together with
the code that emits it. `E0282` keeps rustc's number for "type annotations needed"
because it is widely recognized; it is allocated in the E02xx block as an exception.

### 11. Testing

- `spec/conformance/` gains run cases (`.loom` + `.stdout`, optionally `.stderr` and
  `.exit`) and reject cases (`.loom` + `.diag`) for every Phase 2 spec section.
  `cargo xtask conformance` runs both.
- `cargo xtask examples` runs every example whose phase has been reached and compares
  stdout byte for byte. It is part of `cargo xtask ci`.
- `--dump=hir`/`--dump=types` snapshots for each lowering and inference rule.
- Every new diagnostic has a reject case and a snapshot of its rendered output.

## What Rust does

Rust's pipeline is AST → name resolution → HIR → type check (`rustc_hir_typeck`) → MIR,
with resolution side tables, bidirectional checking plus unification, two-pass argument
checking, Maranget's usefulness algorithm and the orphan rule. Loom adopts all of these.
The differences follow from Loom's surface. Paths use `.` for modules, types and values,
so resolution hands the tail of a path to the checker (Rust separates `::` from `.`).
Identifier patterns are classified by capitalization rather than by what is in scope,
which removes Rust's "a binding silently shadowed a constant" bug class. `Fn(A) -> R` is a
type rather than only a bound, so higher-order code needs neither `impl Fn` nor
`Box<dyn Fn>`. Default and named arguments exist. Overflow traps in every profile.
rustc's const evaluator interprets MIR (Miri); Loom's interpreter is a first-class
oracle from the start.

## Alternatives considered

- **Salsa now:** rejected for Phase 2. It is a large dependency and the compiler has no
  incremental consumer until the language server. The query-shaped API keeps the option.
- **Built-in types implemented entirely in Rust inside the checker:** rejected. Every method
  signature would be Rust code, invisible to users and to `loom doc`, and duplicated by
  each backend. Core in Loom plus a small intrinsic list is how Rust's `core` works too.
- **Global or HM-style inference across functions:** rejected by ADR 0004.
- **Generic trait `Iterator[T]` instead of an associated type:** simpler to implement, but
  it lets one type implement `Iterator` twice and makes inference ambiguous. Associated
  types as decided in spec §3.4.
- **Classifying identifier patterns by scope (Rust):** rejected as above.
- **Copying values on every move in the interpreter:** simplest, but quadratic on
  common code. Copy-on-write gives the same observable semantics.
- **Monomorphizing before interpretation:** unnecessary for an interpreter; passing the
  substitution is simpler and gives the same results.

## Consequences

- The core library becomes the largest Loom codebase in the repository and the first real
  test of the language. Its method bodies are good Jules tasks once signatures exist.
- Phase 2 programs can violate ownership rules without an error; the interpreter still
  runs them. Phase 3's borrow checker will reject some programs that run today. This is
  stated in `docs/STATUS.md` so nobody relies on it.
- `ref T` being transparent in Phase 2 means some type errors involving references will
  only be reported in Phase 3.
- Capitalization becomes significant in patterns, so the style guide and lints must
  enforce `UpperCamelCase` types and variants, `SCREAMING_CASE` constants and
  `snake_case` bindings.
