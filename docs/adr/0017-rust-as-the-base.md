# ADR 0017: Rust as the base — where Loom matches Rust and where it deliberately differs

- **Status:** Accepted (living document: append a row whenever an ADR or RFC adds a difference)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** every other ADR

## Context

Rust is Loom's foundation in implementation, semantics, ecosystem and process
([vision](../vision.md)). Every ADR asks "what does Rust do here, and why?". This ADR
collects the answers in one place, so a contributor from the Rust world can learn Loom
quickly, and so differences stay deliberate rather than accidental.

## Decision

**Default rule:** where this table is silent, Loom does what Rust does. A new difference
requires an ADR or RFC that adds a row here.

### Matches Rust (semantics are the same)

| Area | Shared semantics |
|---|---|
| Ownership | Single owner, moves, deterministic `Drop`, explicit `clone`, `Copy` types |
| Borrowing | Many readers XOR one writer, checked on MIR with NLL |
| Safety | No UB in safe code; `unsafe` has Rust's meaning |
| Errors | `Option`, `Result`, `?` with `From`, panics for bugs |
| Data types | Structs, enums with payloads, tuples, exhaustive pattern matching |
| Traits | Traits, generics with bounds, monomorphization, `dyn Trait`, coherence and orphan rule |
| Concurrency safety | `Send`/`Sync` auto-traits |
| Shared ownership | `Shared[T]` = `Arc<T>`, `Mutex[T]` owns its data |
| Process | Editions, RFCs, stability attributes, the rustc-style compiler pipeline |
| Toolchain | A single tool (`loom` ≈ cargo + rustc + rustfmt + clippy + rust-analyzer) |

### Deliberate differences

| Area | Rust | Loom | Why | ADR |
|---|---|---|---|---|
| Surface syntax | Braces, `;`, `let`, `::`, `&&` | `def … end`, newlines, `x =`/`var`, `.`, `and`/`or`/`not` | Readability (Ruby feel) | 0003 |
| Lifetimes | Written when elision fails; refs in structs | Never written; refs are second-class; borrowed returns only under elision rules | Learning curve | 0005 |
| Reference syntax | `&T`, `&mut T`, `&x` at call sites | Parameter modes (default borrow, `mut`, `own`); call sites unmarked | Readability | 0005 |
| Closures | `move` keyword, escaping by default | Non-escaping by default (Swift); `own` parameters escape and captures move automatically | Fewer annotations | 0005 |
| Methods | Separate `impl Type` blocks | Inherent methods inside the type body | Ruby feel, no open classes | 0003 |
| Overflow | Checked in debug, wraps in release | Traps in every profile | Correctness over speed | 0004 |
| Async | `async`/`await` + library executors | Built-in M:N tasks, no coloring, structured scopes | Simplicity (Go model) | 0006 |
| Panics | Unwind or abort (profile) | Always unwind to the task scope | Structured concurrency needs it | 0007 |
| `Rc` | `Rc` and `Arc` | Only `Shared` (atomic) in safe std 1.0 | One obvious choice; tasks migrate between threads | 0005 |
| Dependency resolution | Max-version SAT | Minimal version selection | Reproducibility | 0010 |
| Generic syntax | `<T>` | `[T]` | Avoids `<`/`>` ambiguity, reads cleanly | 0004 |
| Attributes | `#[…]` | `@[…]` | `#` is the comment character | 0003 |
| Macros | `macro_rules!`, proc macros | None pre-1.0; built-in derives only | Readability, tooling, compile speed | 0003 |
| Integer default | Inferred `i32` fallback | `Int` = 64-bit | Fewer overflow surprises | 0004 |

## What Rust does

This document *is* the comparison.

## Alternatives considered

- **Scattering the comparison across ADRs only:** rejected. Contributors need one table.

## Consequences

- Reviewers reject unexplained divergence from Rust.
- The "Loom for Rust developers" guide (docs, Phase 6) is generated largely from this table.
