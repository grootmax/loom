# Vision

Loom is a statically typed, compiled, memory-safe language that **reads like Ruby, is as
safe and fast as Rust in practice, and does concurrency as simply as Go.** It ships as a
single `loom` tool that builds, tests, formats, documents and packages code, produces
small native binaries for Linux, macOS and Windows (and later WebAssembly, iOS and
Android), and is the foundation for official web, desktop, mobile and cloud frameworks.

It is meant to be a language people can still build on in a hundred years. That goal
shapes how we work: clarity over cleverness, stability over features, a written
specification over implementation behavior, and few dependencies over convenient ones.

## What we take from each parent language

| From | Take | Deliberately leave behind |
|---|---|---|
| **Ruby** | `def … end` blocks, `do \|x\| … end` and `{ \|x\| … }` closures, `"#{x}"` interpolation, keyword arguments, expressive collections, readable English-like code, programmer happiness | Monkey-patching, `method_missing`, open classes, `nil` everywhere, runtime-only errors, a global interpreter lock |
| **Rust** (the base) | Implementation language; ownership, moves and borrowing; deterministic destruction; `Option`, `Result` and `?`; algebraic data types; exhaustive matching; traits; generics; memory and data-race safety; crate interop; excellent diagnostics; editions | Lifetime annotations in everyday code, long compile times, a steep learning curve |
| **Go** | Lightweight tasks, channels, `select`, no function coloring, one canonical formatter, fast builds, static binaries, cross-compilation by flag, a big useful standard library, a strong compatibility promise | `if err != nil` boilerplate, nil interfaces, unstructured goroutine leaks |

## Rust is the base

Rust is not just the language the compiler is written in. It is Loom's foundation at
every layer:

- **Implementation.** Compiler, runtime, scheduler, the native layer of std, package
  manager, formatter, linter and language server are written in Rust.
- **Semantics.** Ownership, moves, borrowing, `Drop`, `Option`/`Result`, traits,
  monomorphized generics, exhaustive `match`, `Send`/`Sync`. Loom's job is to make these
  *feel like Ruby*: ownership and borrows are inferred, and lifetimes never appear in
  everyday code.
- **Ecosystem.** Loom code can use Rust crates through a generated bridge, and Rust
  programs can embed Loom.
- **Process.** Editions, RFCs, a stability promise, world-class diagnostics, one tool.

The first question in every design decision is: *what does Rust do here, and why?* Loom
deviates only to be simpler to read and write, and records why.

## Who Loom is for

- Ruby and Python developers who want speed, static types and simple deployment without
  learning lifetimes.
- Go developers who want sum types, generics with traits, and no nil.
- Rust developers who want a faster-to-write language for application code that still
  interoperates with their crates.

## What success looks like

- A newcomer writes a correct concurrent network service in their first week.
- Code written for Loom 1.0 still compiles, unchanged, decades later.
- The specification is precise enough that a second, independent implementation could pass
  the conformance suite.
