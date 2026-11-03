# ADR 0022: Concurrency runtime (Phase 4)

- **Status:** Accepted (provisional; revisited at the end of Phase 4)
- **Date:** 2026-10-29
- **Deciders:** lead architect
- **Related:** [ADR 0006](0006-concurrency-model.md), [ADR 0005](0005-ownership-and-borrowing.md),
  [ADR 0021](0021-native-backend-and-runtime.md), spec §11

## Context

ADR 0006 fixed the model: lightweight tasks without function coloring, structured
concurrency with `scope`, channels, `select`, cooperative cancellation, `Send`/`Sync`.
Phase 4 must implement it in the runtime, the native backend and the interpreter, and
meet three exit criteria: a benchmark with one million concurrent tasks, race tests under
a deterministic scheduler, and a published TCP echo benchmark. This ADR fixes the runtime
design and the parts of ADR 0006 it left open (stacks, cancellation results, `select`
details, panics in tasks).

## Decision

### 1. Tasks are stackful coroutines

- A task runs on its own stack: a virtual-memory reservation with a guard page below it
  (`mmap` with `MAP_NORESERVE` on Unix, `VirtualAlloc` on Windows), committed lazily by
  the OS on Unix and up front on Windows. The default reservation is 256 KiB for native
  tasks and 64 MiB for interpreter tasks (the tree-walking interpreter recurses deeply);
  `LOOM_TASK_STACK` overrides it. Freed stacks go to a per-worker cache.
- Switching tasks saves the callee-saved registers and the stack pointer and loads
  another task's: about twenty instructions of hand-written assembly per platform ABI
  (System V x86-64, Windows x64 including the TIB stack bounds and XMM6–15, AAPCS64 for
  Linux and Apple), as `#[unsafe(naked)]` functions in `runtime/`.
- Guard pages turn stack overflow into a fault. The runtime installs a handler for it
  (`sigaltstack` + `SIGSEGV` on Unix, a vectored exception handler on Windows) that
  prints `panicked: stack overflow in task` and exits with status 101.
- Growable copying stacks (Go) need precise stack maps, which Cranelift does not
  provide; segmented stacks need a stack check in every prologue. A million tasks at one
  touched page each is 4 GiB on Linux (4 KiB pages) — acceptable for the benchmark, and
  the reservation size is a knob.

### 2. Scheduler

- **Parallel mode** (default for native code): one worker OS thread per core
  (`LOOM_WORKERS` overrides), each with a local run queue, plus a global injection queue.
  An idle worker steals half of another worker's queue, then parks on a condition
  variable. A task may resume on a different worker than the one it parked on.
- **Deterministic mode** (`LOOM_SCHED=deterministic[:SEED]`, and always in the
  interpreter): one worker; at every scheduling point it picks the next ready task with a
  seeded pseudo-random generator, and time is virtual: when every task is blocked, the
  clock jumps to the next timer. A seed reproduces an interleaving exactly, which is what
  race tests need; running a test across many seeds explores interleavings.
- Scheduling points are the blocking operations: channel `send`/`recv`, `select`,
  `sleep`, joining a scope, locking a mutex, I/O, and `yield_now`. There is no
  preemption in Phase 4; a task that loops without blocking keeps its worker (other
  workers keep running). Preemption at loop back-edges is a later option.
- `main` runs as the first task. The program ends when `main` returns (detached tasks
  are abandoned), when `main` returns `Err`, or at the first panic.

### 3. Structured concurrency

- `scope(deadline: d) do |s| … end` (the deadline is optional) runs the block with a
  scope handle and returns the block's value; `s.spawn { … }` starts a child and returns
  a `Task[T]` whose `join` waits for the child and returns its result. Leaving the block
  waits for every child. Children may
  borrow from the enclosing function: the borrow checker treats each spawned closure as a
  borrow that lives until the end of the `scope` call, so two children cannot both
  borrow the same binding mutably (E0502).
- **Panics.** Loom has no `recover`. A panic in any task prints its message and ends the
  program with status 101, as a panic in `main` does (spec §10.4). This is what ADR
  0006's "cancels its siblings and re-raises in the parent" amounts to when nothing can
  catch the re-raised panic, and it needs no unwinding. Drops do not run on panic
  (ADR 0021 §5); unwinding stays a later improvement.
- **Cancellation** is cooperative. `s.cancel` or an expired deadline cancels the scope
  and its descendants. In a cancelled scope, blocking operations return at once with a
  "cancelled" result instead of waiting: `recv` returns `None`, `send` returns `false`
  (dropping the value), `sleep` returns `false`, a `select` without an `else` arm
  behaves as if every `recv` arm received `None`. `s.cancelled` reports the state. The
  scope still waits for its children, which notice cancellation at their next blocking
  operation. ADR 0006 named the result `Err(Cancelled)`; returning the existing
  "nothing" values keeps `recv` an `Option` and spares every caller a second error type.
- **Detached tasks:** `Task.detach { … }` takes an `own` closure that must be `Send` and
  borrows nothing. Detached tasks are abandoned when `main` ends.

### 4. Channels and `select`

- `Chan[T].new(capacity: Int = 0)`: `0` is a rendezvous channel. `send(own value) ->
  Bool` blocks while the channel is full (or, for capacity 0, until a receiver takes the
  value); sending on a closed channel panics. `recv -> Option[T]` blocks while the channel
  is empty and open; it returns `None` once the channel is closed and drained.
  `close` is idempotent. `try_recv -> Option[T]` never blocks. A channel is shared by
  borrowing it (it is `Sync`); values sent through it must be `Send`.
- `select` waits until one arm can proceed. A `recv` arm (`when P = ch.recv`) is ready
  when a value is available or the channel is closed, and matches `P` against the
  `Option[T]` result; a `send` arm (`when ch.send(v)`) is ready when the send would not
  block; a timer arm (`when Time.after(d)`) is ready after `d`. If several are ready, one
  is chosen uniformly at random (Go). An `else` arm runs when no arm is ready.
  Implementation: the selecting task enqueues a waiter with a shared atomic token on
  every channel; the first channel that wakes it wins the token, and the others discard
  their waiters.

### 5. Time

Module `std.time`: `Duration` (nanoseconds) with the trait `DurationLiterals` providing
`seconds`, `milliseconds`, `microseconds` and `nanoseconds` on `Int` (a trait, not a
reopened `Int`, as example 22 says); `Time.sleep(d) -> Result[Unit, Cancelled]`;
`Time.after(d)` for `select`; `Time.now -> Instant` and `elapsed` on a monotonic clock
(virtual in deterministic mode); `Cancelled`, an error type. Timers live in a binary heap
per scheduler.

Until packages arrive in Phase 5, `std` names the embedded library: `std.time`,
`std.sync` and `std.net` are modules of `std/core/`, and `use std.time.Time` resolves
there. Phase 5 moves them into a separate `std` package without changing these paths.

### 6. I/O reactor

One reactor thread per process waits on epoll (Linux) or kqueue (macOS, BSD) and wakes
tasks whose sockets became ready; sockets are non-blocking, and an operation that would
block registers interest and parks the task. On Windows, Phase 4 performs blocking
socket calls on a small pool of helper threads, which park only the calling task; IOCP
replaces the pool later without changing the API. System calls are declared with
`extern "C"` in the runtime (no `libc` crate; the C library is already linked).
Phase 4's networking surface is what the echo benchmark needs: `net.TcpListener.bind`,
`accept`, `TcpStream.connect`, `read`, `write_all`, `close`. The rest of `std.net`
belongs to Phase 5.

### 7. `Send` and `Sync`

Core lang traits `Send` and `Sync`, implemented automatically and structurally: a type is
`Send` (`Sync`) when all its fields are. Primitives, `String`, `Array`, `Map`, `Chan`,
`Mutex` and closures whose captures are `Send` are `Send`; borrowed types `ref T` are
`Send` when `T` is `Sync`. The checker requires `Send` for values sent on channels, for
the captures of `Task.detach` closures, and for the captures of spawned closures that
move into the child, and `Sync` for bindings a spawned closure borrows. Every type of this
edition is `Send` and `Sync`, so the checks take effect with the first types that are not
(foreign handles, Phase 5); spec Appendix C reserves E0600 and E0601 for them. A detached
closure cannot borrow because it is escaping (E0503).

### 8. Synchronization and shared ownership

Module `std.sync`: `Mutex[T]` owns its value; `lock { |mut value| … }` runs a closure with
exclusive access and returns its result (the lock is held for the closure's duration, so
no guard can escape). `AtomicInt` with `load`, `store`, `add`, `compare_exchange`.
`WaitGroup` is unnecessary given `scope`. Waiting for a mutex parks the task, not the
worker.

`Shared[T]` (spec §8.5, ADR 0005 §4) is atomically reference-counted shared ownership:
`Shared.new(value)`, `clone` (another owner), `Shared.count(s)`. Methods of `T` are
callable on a `Shared[T]` through the core trait `Deref` (`type Target`, `deref -> ref
Target`): method lookup and field access continue through `Deref` after a type's own
members, as Rust's auto-deref does. `Shared` gives shared access only; mutation goes
through `Mutex` or `AtomicInt`. `Weak` waits until a program needs it.

### 9. Native code and the interpreter

- Native code calls the runtime through the C ABI of ADR 0021: `loom_scope_*`,
  `loom_spawn`, `loom_chan_*`, `loom_select_*`, `loom_sleep`, `loom_mutex_*`, plus
  descriptors for element types. Closures are passed as `{code, env}` pairs; the runtime
  runs them on task stacks. `loom run` and executables start the scheduler and run
  `main` as a task.
- The interpreter uses the same scheduler in deterministic mode on one OS thread, through
  a safe Rust API (`loom_runtime::sched::Local`): its values are not thread-safe, so its
  tasks never leave the thread. The API is scoped like `std::thread::scope`: spawned
  closures may borrow the interpreter, and a scope does not return before its children
  finish.
- Differential testing covers concurrent programs whose output does not depend on the
  schedule (spec conformance rule 4). Schedule-dependent behavior is tested with
  deterministic seeds instead.

### 10. Verification

- Unit tests of the scheduler, channels and `select` in `runtime/`, run under Miri where
  possible (Miri cannot switch stacks; those tests are marked) and under the
  deterministic scheduler with many seeds (a systematic check of interleavings in place
  of the `loom` model checker, which would be a dependency).
- `cargo xtask bench` gains `tasks_1m` (one million tasks blocked on channels, then
  released) and `tcp_echo` (connections × messages per second against a Loom echo
  server), reported in `docs/benchmarks.md`.

## What Rust does

Rust offers OS threads, `std::thread::scope` (whose borrowing rules §3 adopts), and
`async`/`await` with executors such as tokio, whose work-stealing scheduler, reactor
(mio) and timer wheel are the model for §2, §5 and §6. Loom differs in running blocking
code on stackful tasks instead of compiling functions into state machines, which is how
Go works; the cost is a stack per task. `Send`/`Sync` are Rust's auto traits unchanged.
Rust's `Mutex<T>` owns its data as Loom's does, but hands out a guard; Loom uses a
closure because a guard is a borrow that would have to be returned from a function.

## Alternatives considered

- **State-machine tasks (async/await):** rejected by ADR 0006 (function coloring).
- **OS thread per task:** cannot reach a million tasks.
- **Growable copying stacks:** need stack maps that Cranelift lacks.
- **`Err(Cancelled)` results:** a second error type on every blocking call; see §3.
- **Unwinding child panics into the parent:** nothing can catch them; deferred.
- **Depending on `libc`, `mio`, `crossbeam`:** each is reasonable, but the runtime is
  the place we promised to own (ADR 0017); the system-call surface needed is small.

## Consequences

- `runtime/` gains several thousand lines of `unsafe` Rust (stack switching, system
  calls), each block with a `// SAFETY:` comment and tests.
- Deterministic mode makes concurrency bugs in Loom programs reproducible by seed.
- No preemption: a busy loop can starve tasks queued behind it on its worker.
- A panic anywhere ends the program; programs that need isolation use separate processes
  until a later design adds supervised task groups.
