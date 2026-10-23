# ADR 0005: Ownership and borrowing model

- **Status:** Accepted (provisional). **Owner review requested** on the points marked ⚑.
- **Date:** 2026-10-23
- **Deciders:** lead architect; project owner for ⚑ items
- **Related:** [spec §8](../../spec/loom-spec.md), [ADR 0004](0004-type-system-and-inference.md), [ADR 0006](0006-concurrency-model.md), [ADR 0016](0016-rust-interop.md), [ADR 0017](0017-rust-as-the-base.md)

## Context

Loom wants Rust's guarantees, with no use-after-free, no data races and deterministic
destruction, and also no garbage collector. Rust gets these from ownership, borrowing and
lifetimes. Lifetimes are the single biggest source of Rust's learning curve: `'a`
annotations, structs that borrow, and "borrowed value does not live long enough". Loom's
goal is to keep the guarantees and remove the annotations from everyday code.

## Decision

### 1. Ownership (exactly Rust's)

- Every value has one owner. Assignment, passing an argument in `own` mode, and returning a
  value all **move** it. After a move, the source is unusable, and the compiler rejects
  any further use.
- Values are destroyed deterministically, in reverse declaration order, when their owner
  goes out of scope (`Drop`).
- **Copy types** are duplicated instead of moved: all primitive numbers, `Bool`, `Char`,
  `Unit`, and tuples or structs that derive `Copy` (`@[derive(Copy)]`) whose fields are
  all `Copy`. `Copy` is never inferred, because it is a promise in a public API.
- `clone` is explicit (`user.clone`). It is never inserted silently.

### 2. Parameter modes replace reference types

| Mode | Spelling | Meaning | Rust equivalent |
|---|---|---|---|
| borrow (default) | `def f(x: T)` | Read-only access; the caller keeps ownership | `x: &T` (or `T` for Copy types) |
| mutable borrow | `def f(mut x: T)` | Exclusive read-write access; the caller keeps ownership | `x: &mut T` |
| owned | `def f(own x: T)` | The callee takes ownership; the caller's value is moved | `x: T` |

The same modes apply to receivers: `def len`, `def mut push(x: T)`, `def own into_string`.
At the **call site**, the mode is not written: `push(list, 3)`, not `push(&mut list, 3)`.
This is intentional, so that code reads like Ruby. Mutation stays visible where it matters:
a `mut` argument must be a `var` binding or a field reachable through one, so `var` marks
every place that can change.

Rust's borrowing rule applies everywhere: **at any point, a value has either any number of
readers or exactly one writer.** The borrow checker enforces it on MIR using
non-lexical-lifetime analysis (Rust's NLL, and the Polonius-style refinement when available).

### 3. References are second-class, with one escape hatch

Borrows exist at runtime, but are restricted so that lifetimes never need to be written:

1. A borrow may be **passed down** into calls and **used locally**.
2. A borrow may **not be stored** in a struct or enum field, in a collection, or in a
   `Shared`/`Chan` value, and may not be captured by an escaping closure.
3. ⚑ **A borrow may be returned** from a function only when Rust's *lifetime elision rules*
   pick its source unambiguously: a method returns a borrow of `self`, or a function has
   exactly one borrowed parameter. The return type says so with `ref T` or `mut ref T`,
   for example `def get(key: K) -> Option[ref V]` and `def first -> Option[ref T]`. When
   elision is ambiguous, returning a borrow is an error. The fix-it suggests returning an
   owned value (`clone`), an index or key, or `Shared[T]`.
4. **View types** are std types that hold a borrow internally: iterators (`Iter[T]`), slices
   (`Slice[T]`), and string views (`StrView`). They follow the same rules as references.
   They can be returned under rule 3 and chained (`users.iter.filter { … }.map { … }`), and
   they cannot be stored in fields or escape their source's scope. User-defined view types
   are post-1.0 and need an RFC.
5. **Closures follow the same rules.** A closure parameter is **non-escaping by default**
   (like Swift's), so it may borrow freely. A parameter that stores or detaches the closure
   declares it `own` (`def on_click(own handler: Fn() -> Unit)`). An escaping closure
   captures by move, and the compiler infers which captures are moved.

Why ⚑ rule 3 exists: the kickoff draft said references "cannot be returned". Strictly
applied, that rule forbids `map.get(key)`, `array.first`, `string.trim` and every iterator
adapter, so each of them would need to clone. That makes the safe-and-fast pillar
unreachable for ordinary code. Rust's elision rules already cover the large majority of
real-world signatures without annotations, so we adopt exactly those and forbid the rest.
No lifetime is ever written; the cases that would need one are redesigned.

### 4. Shared ownership is explicit

- `Shared[T]` is atomically reference-counted shared ownership (Rust's `Arc<T>`). Cloning
  it is cheap and visible.
- Shared mutable state requires synchronization: `Shared[Mutex[T]]`, `Shared[RwLock[T]]`,
  or `Atomic*`. There is no non-atomic `Rc` in safe std 1.0, because tasks may migrate
  between threads and one type is simpler. Revisit if profiling demands it.
- Cycles leak, as in Rust. A `Weak[T]` is provided for parent links.

### 5. What users do instead of borrowed fields

| Rust pattern needing lifetimes | Loom pattern |
|---|---|
| `struct Parser<'a> { src: &'a str }` | The parser owns its `String`, or the source is passed into each method call |
| A struct holding `&mut Vec<T>` | Pass the collection as a `mut` parameter to each method |
| Graph or tree with parent pointers | Indices into an arena `Array`, or `Shared`/`Weak` |
| Returning a borrow chosen from two inputs | Return an index or enum saying which; or clone |
| Long-lived iterator stored in a struct | Store the collection and a position index |

### 6. Errors teach

Every ownership or borrow error explains the problem in plain English and offers at least
one concrete fix: move the use earlier, `clone`, use `Shared`, restructure, or change a
parameter mode. Real examples are collected in the error index (`loom explain E0xxx`).

### 7. `unsafe`

`unsafe` has Rust's meaning and rules. It is allowed only in `std/`, `runtime/` and FFI
bindings. In official packages, any other use fails lint.

## What Rust does

Rust has first-class references with lifetime parameters that users annotate when elision
does not apply, references in structs, and `&`/`&mut` at call sites. Loom keeps the runtime
model and the checking algorithm unchanged, so a Loom value and a Rust value have the same
layout and drop behavior. That matters for [interop](0016-rust-interop.md). What Loom
removes is the *expressible* surface: no lifetime parameters, no borrowed fields, no `&`
at call sites. Every Loom program is therefore a program Rust's borrow checker would also
accept. The reverse is not true.

## Alternatives considered

Evaluated against fidelity to Rust semantics, ease of learning, performance
predictability, FFI with Rust crates, and implementation complexity:

| Option | Fidelity | Learning | Predictability | Rust FFI | Implementation |
|---|---|---|---|---|---|
| **A. Inferred ownership, second-class refs + elision (chosen)** | High: same model | Moderate: moves must be learned, no lifetimes | High: no hidden counts | Direct: same layouts | High: NLL borrowck, but no lifetime inference across items |
| B. Perceus-style optimized RC by default (Koka, Lean) | Low: values are shared, mutation is copy-on-write | Easiest | Medium: RC traffic and CoW copies are invisible; reuse analysis is fragile | Needs wrapping at every boundary | Medium |
| C. Mutable value semantics only (Hylo/Val) | Medium | Easy | High | Awkward: no references to hand Rust | Medium; research-grade |
| D. Full Rust lifetimes | Exact | Hard: the thing we set out to avoid | High | Direct | Highest |
| E. Tracing GC | None | Easiest | Low: pauses | Hard | Medium |

B was the most serious alternative. It is rejected as the default because it hides costs
(pillar 5), weakens Rust interop, and makes "safe and fast like Rust" depend on optimizer
heuristics. Its lesson, that the compiler can handle the common case, is adopted in our
inferred modes and inferred closure captures.

## Consequences

- Most code never mentions ownership: parameters borrow, `var` marks mutation, and moves
  happen on assignment and `own` parameters.
- Some Rust designs (borrowing parsers, zero-copy structs) are not expressible in safe
  Loom. They need owned data, indices, or a Rust crate behind the bridge.
- ⚑ **Owner decisions requested:**
  1. Confirm rule 3 (elision-based borrow returns) instead of "never return references".
  2. Confirm that opt-in explicit lifetimes (`ref['a] T`) remain *out* of the language until a
     post-1.0 RFC demonstrates need. The keyword `ref` is reserved now so that remains possible.
  3. If Phase 3 user testing shows ownership errors hurting readability too much, the
     fallback is option B for *application* code, behind an edition. That would be a new ADR.
- The borrow checker conformance suite (Phase 3) must cover accept *and* reject cases for
  every rule in this ADR.
