# ADR 0004: Type system and inference boundaries

- **Status:** Accepted (provisional — revisited at the end of Phase 2)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [spec §5–§6](../../spec/loom-spec.md), [ADR 0005](0005-ownership-and-borrowing.md), [ADR 0007](0007-error-handling.md)

## Context

Static types catch errors early and make code navigable, but annotations everywhere make
code noisy, and global inference (Hindley-Milner across functions) produces errors far
from their cause and makes public APIs change by accident.

## Decision

1. **Static, strong, locally inferred.** Types are required on function parameters, function
   return types (unless `Unit`), struct fields, enum payloads, constants, and statics. Inside
   function bodies, types are inferred, with bidirectional checking plus unification within a
   single function body. Inference never crosses a function boundary, so every error is local.
2. **Primitives:** `Int` (an alias of `I64`), `I8 I16 I32 I64 I128`, `U8 U16 U32 U64 U128`,
   `ISize USize`, `Float` (an alias of `F64`), `F32`, `Bool`, `Char` (a Unicode scalar value),
   `String` (owned UTF-8), `Bytes`, `Unit`, `Never`.
3. **Built-in generic types:** `Array[T]`, `Map[K, V]`, `Set[T]`, `Option[T]`,
   `Result[T, E]`, `Chan[T]`, `Shared[T]`, `Mutex[T]`, tuples `(A, B)`, and function types
   `Fn(A) -> R`, `FnMut(A) -> R`, `FnOnce(A) -> R`. Generic arguments use square brackets.
4. **User types:** `struct`, `enum` (sum types with optional named or positional payloads),
   `trait`, `impl Trait for Type`, and `type Alias = …`.
5. **Generics:** `def max[T: Ord](items: Array[T]) -> Option[T]`, with `where` clauses for
   longer bounds. Generics are monomorphized. `dyn Trait` gives opt-in dynamic dispatch and
   is always visible in the type.
6. **Trait coherence:** Rust's orphan rule. An `impl` must live in the crate that defines
   the trait or the type.
7. **Mutability:** bindings are immutable unless declared `var`. Method receivers are borrowed
   immutably by default. `def mut push(x: T)` takes `self` mutably and `def own into_bytes`
   consumes `self` — the same `mut`/`own` modes used for parameters
   ([ADR 0005](0005-ownership-and-borrowing.md)). A value reached through an immutable binding
   or borrow cannot be mutated.
8. **No null, no exceptions.** `Option` and `Result` ([ADR 0007](0007-error-handling.md)).
9. **Integer overflow traps in every build profile.** Arithmetic that overflows panics, in
   both debug and release. Explicit alternatives are always available: `wrapping_add`,
   `saturating_add`, `checked_add` (which returns `Option`), and `overflowing_add`. Integer
   division by zero panics. Float arithmetic follows IEEE 754.
10. **Numeric conversions are explicit** (`x.to_i32()` returns a `Result`, `x.as_f64()` is
    lossless-or-documented). There are no implicit widening conversions, except that integer
    literals take the type their context expects.
11. **Uniform access:** `value.name` resolves to a field if one exists, otherwise to a
    zero-argument method.

## What Rust does

Rust also requires signature annotations and infers locally. That is the main reason its
errors are local, so Loom keeps it. Rust has `&`/`&mut` types, which Loom replaces with
parameter modes ([ADR 0005](0005-ownership-and-borrowing.md)). **Deliberate difference:** Rust
checks overflow in debug and wraps in release. Loom traps in both, as Swift does.
Correctness is pillar 1 and performance is pillar 5. A program whose meaning changes with
the build profile is a correctness hazard, and measured costs of overflow checks in
optimized code are typically a few percent. Code that needs wrapping says so explicitly.

## Alternatives considered

- **Global type inference (OCaml, Crystal):** rejected. Errors appear far from their cause,
  and accidental API changes become possible.
- **Gradual typing:** rejected. Its "runtime-only errors" are what we leave behind from Ruby.
- **Rust's debug-check/release-wrap rule:** rejected as described above. We may revisit it
  if Phase 7 benchmarks show significant cost, but the default stays checked.
- **Arbitrary-precision `Int` (Python, Ruby):** rejected. Unpredictable performance and
  allocation; `BigInt` belongs in std.

## Consequences

- Public APIs are self-documenting, and IDE inlay hints show the inferred local types.
- Overflow checks must be optimized well (range analysis in `loom_opt`, LLVM in release).
- `Int` is 64-bit on every target, including 32-bit wasm, for portability. `USize` is the
  platform-dependent type.
