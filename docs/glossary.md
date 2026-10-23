# Glossary

Terms used across Loom's docs, spec and code. Keep entries short; link to the defining
ADR or spec section.

| Term | Meaning |
|---|---|
| **ADR** | Architecture Decision Record: a short document recording one decision, its context, alternatives and consequences. See `docs/adr/`. |
| **AST** | Abstract syntax tree: typed views over the CST without trivia. |
| **Borrow** | Temporary access to a value without taking ownership. Immutable borrows may be many; a mutable borrow is exclusive. ([ADR 0005](adr/0005-ownership-and-borrowing.md)) |
| **Borrow checker** | The compiler pass, run on MIR, that enforces borrowing rules. |
| **Copy type** | A small type duplicated on assignment instead of moved (integers, floats, `Bool`, `Char`, small plain structs that opt in). |
| **CST** | Concrete syntax tree: a lossless tree that keeps every byte of the source, including whitespace and comments. |
| **Drop** | Deterministic destruction of a value when its owner goes out of scope. |
| **Edition** | A named, dated set of language rules (e.g. edition 2027). Code of different editions links together. ([ADR 0011](adr/0011-editions-and-compatibility.md)) |
| **HIR** | High-level IR: the desugared, name-resolved program used for type checking. |
| **Inferred ownership** | Loom's model: Rust's ownership semantics, with parameter modes and borrows inferred so users rarely annotate them. |
| **Interpreter (reference)** | The executable model of the spec; native backends are tested against it. |
| **MIR** | Mid-level IR: a control-flow graph used for borrow checking, drop elaboration, optimization and code generation. |
| **Monomorphization** | Compiling a separate copy of a generic function for each concrete type it is used with. |
| **Move** | Transfer of ownership; the source can no longer be used. |
| **MVS** | Minimal version selection: choose the minimum version satisfying every requirement (Go's algorithm). ([ADR 0010](adr/0010-modules-and-packages.md)) |
| **Parameter mode** | How a function receives an argument: borrowed (default), `mut` (exclusive borrow), or `own` (ownership transfer). |
| **Panic** | An unrecoverable bug signal. Unwinds to the nearest task scope; not used for expected errors. |
| **RFC** | Request for Comments: the proposal process for language and std changes. See `docs/rfcs/`. |
| **Scope (task)** | A structured-concurrency block that waits for all tasks spawned in it. |
| **Second-class reference** | A borrow that can be passed down and used locally but not stored in a field or returned, so no lifetime annotations are needed. |
| **Send / Sync** | Marker traits: `Send` values may move to another task; `Sync` values may be shared between tasks. Inferred by the compiler. |
| **`Shared[T]`** | Explicit, atomically reference-counted shared ownership (Rust's `Arc<T>`). |
| **Spec** | `spec/loom-spec.md`, the normative definition of the language. |
| **Task** | A lightweight thread of execution scheduled M:N onto OS threads by the runtime. |
| **Tier** | A platform support level with a defined promise. ([ADR 0012](adr/0012-platform-tiers.md)) |
| **Trivia** | Whitespace and comments: kept in the CST, dropped from the AST. |
