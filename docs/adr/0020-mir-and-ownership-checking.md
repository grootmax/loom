# ADR 0020: MIR and ownership checking (Phase 3)

- **Status:** Accepted (provisional; revisited at the end of Phase 3)
- **Date:** 2026-10-29
- **Deciders:** lead architect; owner decisions of 2026-10-29 on ADR 0005 applied
- **Related:** [ADR 0005](0005-ownership-and-borrowing.md), [ADR 0008](0008-compiler-architecture.md),
  [ADR 0009](0009-backend-strategy.md), [ADR 0019](0019-semantic-analysis-and-interpreter.md),
  spec §5.6, §8

## Context

Phase 3 adds a mid-level IR (MIR), the ownership and borrow checker, move and drop
elaboration, and the Cranelift backend. ADR 0005 fixes the model: Rust's ownership, with
second-class references, parameter modes instead of `&`, borrow returns only by Rust's
elision rules (confirmed by the owner), and no written lifetimes (kept out until an RFC).
Phase 2 checked `ref T` as `T` (ADR 0019 §3 rule 2). This ADR decides what changes so
that the model is enforced soundly, including through generic code, and how the checker
works. The native backend and runtime are ADR 0021.

## Decision

### 1. `ref T` becomes a real type

Checking `ref T` as `T` is unsound with generics: `words.iter.to_array` would build an
`Array` of borrows of `words`, and nothing would stop it from outliving `words`. So from
Phase 3:

1. `TyKind::Ref { mutable, inner }` is a type. It is written only in return types (E0302
   elsewhere is unchanged), but generic parameters may be *instantiated* with it
   (`Option[ref T]`, `Array[ref String]`, the `Item` of a view iterator).
2. **Auto-deref.** A value of type `ref T` can be used wherever a borrowed `T` is enough:
   method calls and field reads, operators, interpolation, pattern matching, and as an
   argument to a borrow-mode or `mut`-mode parameter of type `T`. Where an owned `T` is
   needed (a binding annotated `T`, an `own` parameter, a field, a return of `T`), a
   `ref T` coerces to `T` only if `T: Copy`. Otherwise it is error E0501 with the fix-it
   `.clone`.
3. A type is **borrowed** if it contains `ref` or a *view type* (§4) anywhere after
   substitution. Borrowedness follows types through generics: `Stack[ref String]` is
   borrowed, so its values are second-class too.

### 2. MIR

`loom_mir` builds, per function body, a control-flow graph in Rust's MIR shape: locals
(arguments, user bindings, temporaries), basic blocks of statements
(`Assign(place, rvalue)`, `StorageLive/Dead`) and a terminator (`Goto`, `SwitchInt`,
`Call`, `Return`, `Drop`, `Panic`, `Unreachable`). Places are a local plus projections
(field, variant downcast, index). Operands are `Copy(place)`, `Move(place)` or constants.
MIR is generic (polymorphic); backends monomorphize it. It is built from HIR and
`TypedBody`, never from the AST, and its textual form (`loom check --dump=mir`) is
documented in `docs/internals/mir.md` as the contract between the front end and
backends (ADR 0009 rule 2).

### 3. Moves (spec §8.1)

1. Copy types: the primitives, tuples of Copy types, structs and enums deriving `Copy`,
   `ref T` (a shared borrow is copied), and function items. Everything else moves.
2. **Owning positions** move a non-Copy place: a binding's initializer, an assigned
   value, an `own` argument or receiver, a returned value, a field or element of a
   constructed value, the iterated value of `for`, and a capture of an escaping closure.
   Borrow-mode and `mut` arguments, receivers, operators, interpolation, field reads and
   `case` scrutinees do not move.
3. Using a place after it was moved on some path is E0500, which names the move. A
   partial move (a field) makes the whole value unusable except for its other fields.
   Assigning a new value to a moved `var` makes it usable again.
4. Moving out of a borrowed place (a field of a borrowed parameter or `self`, a pattern
   binding of a borrowed value, an element reached by indexing) is E0501 unless the type
   is Copy; the fix-it is `.clone`.
5. **Pattern binding modes are inferred** (spec §9.1 rule 6): matching a place binds
   borrows of its parts, unless a non-Copy binding is used in an owning position in its
   arm; then the binding moves the part out of the place, which must be owned (E0501
   otherwise), and the place counts as (partially) moved after the `case`. Matching a
   temporary always binds by move.

### 4. Borrows (spec §8.2–§8.4)

1. Every value of a borrowed type carries a set of **loans**: (place, shared or
   mutable). Passing a place to a borrow-mode or `mut` parameter creates a loan that
   lasts for the call. The result of a call carries the loans of every argument of a
   borrowed type, plus, if the return type contains `ref`, a new loan on the place passed
   as the **elided source**: `self` for a method, or else the only parameter that is in
   borrow or `mut` mode and not of a Copy type. A signature that returns `ref` without a
   source is E0504.
2. A loan is live while any local that carries it is live (non-lexical liveness on MIR,
   as Rust's NLL). While a shared loan on a place is live, the place and every place
   overlapping it may be read but not written, moved or borrowed mutably; while a mutable
   loan is live, it may not be used at all. A conflict is E0502, pointing at the
   conflicting use, with the loan's origin as a secondary label.
3. Within one call, a `mut` argument must not overlap any other argument (E0502).
4. **Second-class rule.** A value of a borrowed type may be held in locals, passed as an
   argument, returned (its loans then must come from the elided source or from
   borrowed-type arguments), and placed in values whose type is itself borrowed. It may
   not be stored in a field or element of a non-borrowed value, passed to an `own`
   parameter of a non-borrowed type, or captured by an escaping closure (E0503).
5. **View types** are core structs marked `@[view]` (array, map and string iterators):
   their fields may hold borrows, every instance is borrowed, and a method returning a
   view type is a borrow return for rule 1. User-defined view types need an RFC
   (ADR 0005 §3 rule 4).
6. **Closures.** A closure literal in an owning position (an `own` argument, a returned
   value, a field or element) is *escaping*: it captures by move (Copy captures are
   copied; mutating a capture mutates the closure's copy) and its type is not borrowed.
   Any other closure captures by reference, its value carries loans on its captures
   (mutable loans for the bindings it mutates), and it is second-class. A
   non-escaping closure stored in a local and later moved to an owning position is E0503
   with the help "pass the block directly, or capture by move with an `own` parameter".

### 5. Drops (spec §8.6)

1. A value is dropped when its owner goes out of scope without having been moved:
   locals in reverse order of declaration at the end of their block, temporaries at the
   end of their statement, and the old value of a place when it is assigned. A partially
   moved value drops its remaining fields.
2. Dropping a struct or enum calls its `Drop.drop` (core trait `Drop`, `def mut drop`),
   then drops its fields in declaration order. Collections drop their elements in order.
3. Drop elaboration on MIR inserts `Drop` terminators with drop flags where a value is
   moved on some paths only.
4. A panic unwinds and runs the drops of every frame it leaves (ADR 0007). In Phase 3
   the native backend aborts after printing the panic, without running drops; unwinding
   arrives with the concurrency runtime in Phase 4. This is listed in `docs/STATUS.md`.

### 6. The interpreter stays on HIR

ADR 0008 §7 said the interpreter would move to MIR in Phase 3. It stays on HIR: an
oracle that shares MIR with the backends cannot catch MIR-lowering bugs, and differential
testing is the point of having one. The interpreter gains `Drop` (§5): the ownership
checker exports each body's moved places, scope ends and drop order, and the interpreter
runs drops from that table. This amends ADR 0008 §7.

### 7. Diagnostic codes

E0500 use after move; E0501 move out of a borrow; E0502 conflicting borrow or use while
borrowed; E0503 borrow escapes (stored, captured by an escaping closure, or passed to
`own`); E0504 `ref` return without an elided source. Each comes with a reject case, an
accept case of the nearest valid program, and an explanation for `loom explain`.

## What Rust does

This is Rust's model minus the expressible surface. rustc's MIR, NLL liveness, move
paths, drop flags and drop elaboration are the reference for §2, §3 and §5. Where Rust
users write lifetimes, Loom computes loans from elision and from borrowed-type
arguments, which is what Rust's elision rules plus variance give for the common
signatures. Loom's generic rule (§4 rule 1: a result carries the loans of all
borrowed-type arguments) is coarser than an explicit `'a`, so a few programs Rust accepts
with annotations are rejected here; they are the cases ADR 0005 §5 redesigns. Inferred
pattern binding modes replace Rust's `ref`/`&` match ergonomics, and inferred closure
capture modes replace `move`.

## Alternatives considered

- **Keep `ref T` transparent and check borrows per function only:** unsound through
  generic code, as shown above.
- **Rust's region inference with hidden lifetime variables:** exact, but every error
  would be phrased in terms of lifetimes users cannot see or write.
- **Moving the interpreter to MIR:** simpler to maintain, but removes the independent
  oracle.
- **Rust's binding modes (move by default when matching an owned place):** surprising
  for the common "look into an `Option` and keep using it" code; inference keeps such
  programs valid without a `ref` keyword in patterns.

## Consequences

- The Phase 2 type checker learns `ref` types and their coercions; some Phase 2 programs
  that moved out of borrows (for example returning a field of a borrowed parameter) are
  now rejected and must clone. The core library is audited under these rules, and its
  iterators become view types.
- Every rule above gets borrow-checker conformance cases (Phase 3 exit criterion).
- Error messages talk about moves, loans and where each started, never about lifetimes.
