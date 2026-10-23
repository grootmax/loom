# ADR 0006: Concurrency model

- **Status:** Accepted (provisional — details of `select` and cancellation finalized in Phase 4)
- **Date:** 2026-10-23
- **Deciders:** lead architect
- **Related:** [spec §11](../../spec/loom-spec.md), [ADR 0005](0005-ownership-and-borrowing.md), [ADR 0007](0007-error-handling.md)

## Context

Go showed that lightweight tasks, channels and blocking-style code make concurrent programs
easy to write. It also showed the costs: goroutine leaks, data races, and nil channels.
Rust showed that data races can be compile errors, but its `async`/`await` split
("function coloring") doubles API surfaces and confuses newcomers.

## Decision

1. **Tasks.** `spawn` creates a lightweight task (green thread) with a growable stack,
   scheduled M:N onto OS threads by a work-stealing scheduler in `runtime/` (written in Rust).
   Stack strategy (copying vs. segmented vs. guard-paged virtual reservation) is decided by a
   Phase 4 ADR. The current preference is virtual reservation with guard pages and no
   copying, which keeps interior pointers valid for FFI.
2. **No function coloring.** Any function may block. Blocking I/O parks the *task*, not the
   OS thread. The reactor uses epoll (Linux), kqueue (macOS/BSD) and IOCP (Windows), with
   io_uring as a later optimization. There is no `async` or `await` keyword.
3. **Structured concurrency.** Tasks are spawned inside a `scope`:
   ```loom
   scope do |s|
     s.spawn { work(1) }
     s.spawn { work(2) }
   end   # waits for both; a panic in a child cancels its siblings and re-raises here
   ```
   A scope does not finish until all its children finish. Because children cannot outlive
   the scope, they **may borrow** from the enclosing function (Rust's `thread::scope`). That
   fits the second-class reference rules in ADR 0005.
4. **Cancellation and deadlines** are built into scopes: `scope(deadline: 5.seconds) do |s|`,
   plus `s.cancel`. Cancellation is cooperative. It is delivered at blocking points
   (channel operations, I/O, sleeps), where the operation returns `Err(Cancelled)`.
5. **Detached tasks** need `Task.detach { … }`, whose closure is `own` (escaping) and must
   be `Send`. Leaks are therefore visible in review and greppable.
6. **Channels.** `Chan[T].new` creates an unbuffered (rendezvous) channel, and
   `Chan[T].new(capacity: n)` a buffered one. `send` blocks when the channel is full, and
   `recv` returns `Option[T]` (`None` once closed and drained). A channel is closed
   explicitly with `close`; sending on a closed channel panics, as in Go, because it is a
   bug. Channels are `Sync` and may be shared by borrow within a scope or via `Shared`.
7. **`select`** waits on several channel operations or timers:
   ```loom
   select
   when msg = inbox.recv      then handle(msg)
   when Time.after(5.seconds) then log.info("idle")
   end
   ```
   Semantics are finalized in spec §11 in Phase 4: fairness (uniform random among ready
   arms, as in Go), closed-channel behavior, and a non-blocking `else` arm.
8. **Data-race freedom at compile time.** Values that cross into another task must be
   `Send`; values shared between tasks must be `Sync`. Both are auto-traits inferred
   structurally, as in Rust. Users rarely write them, and errors name the field that is
   not `Send`.
9. **Sync primitives** in `std.sync`: `Mutex[T]` (which owns its data, as in Rust),
   `RwLock[T]`, `Atomic*`, `WaitGroup`, `Once`, `Semaphore`.
10. **Testing.** The runtime has a deterministic, seeded scheduler mode for reproducing
    races. Runtime primitives are model-checked (tokio's `loom` crate or an equivalent).

## What Rust does

Rust offers OS threads and `async`/`await` with library executors (tokio and others).
`Send`/`Sync` give compile-time race freedom, which Loom adopts unchanged. Loom
deliberately does not adopt `async`/`await`: the runtime provides what tokio provides,
but beneath ordinary blocking-style functions, as Go does. The cost is a runtime with
its own stacks, which complicates FFI calls that block (see the consequences below).

## Alternatives considered

- **async/await (Rust, C#, JS):** rejected. It causes function coloring and two ecosystems.
- **Unstructured `go` statements:** rejected. Leaks and lost panics; detached tasks remain
  possible but are explicit.
- **Actors only (Erlang, Pony):** rejected. Too restrictive as the sole model; actors can
  be built on tasks and channels.
- **1:1 OS threads only:** rejected. A million tasks is a Phase 4 exit criterion.

## Consequences

- The runtime must handle FFI calls that block for a long time. Like Go's `entersyscall`,
  a thread entering foreign code hands its scheduler slot to another thread.
- Calling Rust `async` crates needs a bridge that drives futures on the Loom reactor
  ([ADR 0016](0016-rust-interop.md)).
- Stack-size and context-switch costs must be benchmarked in CI (Phase 4).
