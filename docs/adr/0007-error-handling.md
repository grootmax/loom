# ADR 0007: Error handling

- **Status:** Accepted
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [spec §10](../../spec/loom-spec.md), [ADR 0006](0006-concurrency-model.md)

## Context

Ruby uses exceptions and `nil`, so errors are invisible in signatures. Go uses error
values but needs `if err != nil` boilerplate, and nil interfaces cause surprises. Rust uses
`Option`/`Result` with `?`, which is explicit and concise, and `panic` for bugs.

## Decision

1. **Absence** is `Option[T]` (`Some(v)` / `None`). There is no null value of any type.
2. **Recoverable failure** is `Result[T, E]` (`Ok(v)` / `Err(e)`). Functions that can fail
   say so in their return type.
3. **`?`** is the postfix propagation operator. On `Err(e)`, it returns `Err(From.from(e))`
   from the enclosing function; on `None`, it returns `None` (in a function returning
   `Option`). It is a compile error elsewhere.
4. **Error types** implement the `Error` trait (`message`, optional `source`). Std provides
   `AnyError`, a boxed, `Send` dynamic error with context chaining, for application code:
   `File.read(path).context("reading config")?`.
5. **`panic`** is used only for bugs: contract violations, `unwrap` of `None`, index out
   of bounds, integer overflow, sending on a closed channel. A panic **unwinds** the current
   task, running `Drop`s, up to the nearest task scope. There the scope cancels the
   siblings and re-raises the panic in the parent. A panic that reaches `main` prints the
   message with a source-located stack trace and exits with status 101.
6. **No `catch`.** Recovering from panics is limited to scope boundaries
   (`scope.try_join` returns `Result[T, Panic]`), for supervisors and servers that must
   isolate request handlers. That is the equivalent of Rust's `catch_unwind`, but structured.
7. **`main`** may return `Unit` or `Result[Unit, E: Error]`. An `Err` prints the error
   chain and exits with status 1.
8. **Exhaustiveness:** `case` on a `Result`/`Option` must handle every variant. Ignoring a
   returned `Result` is a warning (`unused_result`, Rust's `#[must_use]` behavior), and that
   warning is an error in official packages.

## What Rust does

This is Rust's design almost exactly. Rust offers `panic = "abort"` as a profile option.
Loom always unwinds, because structured concurrency depends on scopes observing child
panics. Loom adds `AnyError` (as the `anyhow` crate does) to std so that application code
has one obvious choice.

## Alternatives considered

- **Exceptions (Ruby, Java, Swift's `throws`):** rejected. They are invisible in types, or a
  second effect system.
- **Go-style multiple returns:** rejected. Boilerplate, and nothing forces the error to be checked.
- **Effect handlers (Koka):** powerful, but too novel for a 100-year core.

## Consequences

- The runtime must support unwinding on every Tier 1 target (Cranelift and LLVM both do).
- Std must make `?` ergonomic: `From` conversions, `context`, and `ok_or`.
