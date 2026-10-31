//! The C ABI of the concurrency runtime for native code (ADR 0022 §9).
//!
//! Values cross the boundary as bytes described by a [`TypeInfo`]: a channel moves the
//! bytes of each value into a heap buffer and out again, and drops the values it still
//! holds through the descriptor's drop glue. Closures are `{code, environment}` pairs
//! with the Loom closure convention `code(result, environment, arguments…)`.
//!
//! Handles are reference-counted (`Arc`) runtime objects, so that copying a value that
//! contains one (clone glue) shares it, as in the interpreter.

use super::chan::{Arm, Chan, Selected, SendError, select};
use super::{
    Lock, Scope, Task, enter_scope, exit_scope, join, sleep, spawn_detached, spawn_in, yield_now,
};
use crate::TypeInfo;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

/// The code of a closure with no arguments.
type Code0 = unsafe extern "C" fn(*mut u8, *mut u8);
/// The code of a closure with one (borrowed) argument.
type Code1 = unsafe extern "C" fn(*mut u8, *mut u8, *mut u8);
/// The drop function in a closure environment's header.
type EnvDrop = unsafe extern "C" fn(*mut u8);

/// A heap buffer holding one value of a type described by a [`TypeInfo`].
struct Buf {
    ptr: *mut u8,
    info: *const TypeInfo,
}

// SAFETY: a buffer is owned by one holder at a time and moved between tasks only through
// the runtime's synchronization; the values it holds are `Send` (ADR 0022 §7).
unsafe impl Send for Buf {}

impl Buf {
    /// Moves the value at `src` into a new buffer.
    ///
    /// # Safety
    ///
    /// `src` must hold a value of the described type, which the buffer now owns.
    unsafe fn take(info: *const TypeInfo, src: *const u8) -> Self {
        // SAFETY: descriptors are static data.
        let ti = unsafe { &*info };
        let ptr = crate::loom_alloc(ti.size, ti.align);
        // SAFETY: the buffer has room for the value.
        unsafe { std::ptr::copy_nonoverlapping(src, ptr, ti.size) };
        Self { ptr, info }
    }

    /// Moves the value out to `dst` and frees the buffer.
    ///
    /// # Safety
    ///
    /// `dst` must have room for a value of the described type.
    unsafe fn give(self, dst: *mut u8) {
        // SAFETY: descriptors are static data.
        let ti = unsafe { &*self.info };
        // SAFETY: `dst` has room for the value.
        unsafe { std::ptr::copy_nonoverlapping(self.ptr, dst, ti.size) };
        // SAFETY: allocated by `take` with this size and alignment.
        unsafe { crate::loom_free(self.ptr, ti.size, ti.align) };
        std::mem::forget(self);
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        // SAFETY: descriptors are static data; the buffer holds a value it owns.
        let ti = unsafe { &*self.info };
        if let Some(drop) = ti.drop {
            // SAFETY: the value is initialized and dropped once.
            unsafe { drop(self.ptr) };
        }
        // SAFETY: allocated by `take` with this size and alignment.
        unsafe { crate::loom_free(self.ptr, ti.size, ti.align) };
    }
}

/// Frees a closure environment (null for a function used as a value).
///
/// # Safety
///
/// `env` must be a live closure environment or null, not used afterwards.
unsafe fn drop_env(env: *mut u8) {
    if !env.is_null() {
        // SAFETY: the first word of an environment is its drop function.
        let drop: EnvDrop = unsafe { env.cast::<EnvDrop>().read_unaligned() };
        // SAFETY: the environment is live and dropped once.
        unsafe { drop(env) };
    }
}

/// A closure owned by the runtime (sent to another task).
struct Closure {
    code: Code0,
    env: *mut u8,
}

// SAFETY: the closure's captures are `Send` (ADR 0022 §7); it runs once, on one task.
unsafe impl Send for Closure {}

impl Closure {
    /// Calls the closure, writing its result to `ret`, and frees its environment.
    ///
    /// # Safety
    ///
    /// `ret` must have room for the closure's result.
    unsafe fn call_once(self, ret: *mut u8) {
        // SAFETY: the closure convention; the environment is freed after its only call.
        unsafe {
            (self.code)(ret, self.env);
            drop_env(self.env);
        }
    }
}

fn arc_raw<T>(value: T) -> *mut u8 {
    crate::track(1);
    Arc::into_raw(Arc::new(value)).cast_mut().cast()
}

/// Borrows the `Arc` behind a handle.
///
/// # Safety
///
/// `h` must be a live handle of type `T` made by [`arc_raw`].
unsafe fn arc_ref<'a, T>(h: *mut u8) -> &'a T {
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = h.cast::<T>().cast_const();
    // SAFETY: guaranteed by the caller.
    unsafe { &*p }
}

/// Shares a handle (clone glue).
///
/// # Safety
///
/// `h` must be a live handle of type `T`.
unsafe fn arc_clone<T>(h: *mut u8) -> *mut u8 {
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = h.cast::<T>().cast_const();
    crate::track(1);
    // SAFETY: the handle is live.
    unsafe { Arc::increment_strong_count(p) };
    h
}

/// Releases a handle (drop glue).
///
/// # Safety
///
/// `h` must be a live handle of type `T`, not used afterwards.
unsafe fn arc_drop<T>(h: *mut u8) {
    if h.is_null() {
        return;
    }
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = h.cast::<T>().cast_const();
    crate::track(-1);
    // SAFETY: the handle owned one count.
    drop(unsafe { Arc::from_raw(p) });
}

// ----- scopes and tasks ------------------------------------------------------------------

/// Runs a `scope` block: `code(ret, env, &scope)`; waits for the scope's tasks.
///
/// # Safety
///
/// `code`/`env` must be a closure taking a borrowed `Scope`; `ret` must have room for its
/// result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_scope_run(deadline: i64, code: Code1, env: *mut u8, ret: *mut u8) {
    let (scope, previous) = enter_scope(u64::try_from(deadline).ok());
    let mut handle: *mut u8 = Arc::into_raw(Arc::clone(&scope)).cast_mut().cast();
    // SAFETY: the block receives a pointer to a live scope handle.
    unsafe { code(ret, env, (&raw mut handle).cast()) };
    exit_scope(&scope, previous);
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = handle.cast::<Scope>().cast_const();
    // SAFETY: releases the count taken above.
    drop(unsafe { Arc::from_raw(p) });
}

/// The shared state of a native task: its result, once it has one.
struct TaskState {
    task: Mutex<Option<Arc<Task>>>,
    result: Mutex<Option<Buf>>,
}

/// Starts a child of `scope` running the closure `code`/`env` (which the runtime now
/// owns); its result (described by `info`) is kept for `join`.
///
/// # Safety
///
/// `scope` must be a live scope handle; `code`/`env` a closure with no arguments
/// returning a value described by `info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_scope_spawn(
    scope: *mut u8,
    code: Code0,
    env: *mut u8,
    info: *const TypeInfo,
) -> *mut u8 {
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = scope.cast::<Scope>().cast_const();
    // SAFETY: a scope handle is a live `Arc<Scope>` pointer; this takes one more count.
    let scope_arc = unsafe {
        Arc::increment_strong_count(p);
        Arc::from_raw(p)
    };
    let state = Arc::new(TaskState {
        task: Mutex::new(None),
        result: Mutex::new(None),
    });
    let shared = Arc::clone(&state);
    let closure = Closure { code, env };
    let info_addr = info as usize;
    let task = spawn_in(
        &scope_arc,
        Box::new(move || {
            let info = info_addr as *const TypeInfo;
            // SAFETY: descriptors are static data.
            let ti = unsafe { &*info };
            let ptr = crate::loom_alloc(ti.size, ti.align);
            // SAFETY: the buffer has room for the result.
            unsafe { closure.call_once(ptr) };
            *super::lock(&shared.result) = Some(Buf { ptr, info });
        }),
    );
    *super::lock(&state.task) = Some(task);
    crate::track(1);
    Arc::into_raw(state).cast_mut().cast()
}

/// Cancels a scope.
///
/// # Safety
///
/// `scope` must be a live scope handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_scope_cancel(scope: *mut u8) {
    // SAFETY: the caller passes a live scope handle.
    unsafe { arc_ref::<Scope>(scope) }.cancel();
}

/// Whether a scope is cancelled.
///
/// # Safety
///
/// `scope` must be a live scope handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_scope_cancelled(scope: *mut u8) -> u8 {
    // SAFETY: the caller passes a live scope handle.
    u8::from(unsafe { arc_ref::<Scope>(scope) }.is_cancelled())
}

/// Waits for a task and moves its result to `out`; consumes the handle.
///
/// # Safety
///
/// `handle` must be a live task handle; `out` must have room for the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_join(handle: *mut u8, out: *mut u8) {
    // SAFETY: the caller passes a live task handle.
    let state = unsafe { arc_ref::<TaskState>(handle) };
    let task = super::lock(&state.task).clone();
    if let Some(task) = task {
        join(&task);
    }
    let result = super::lock(&state.result).take();
    if let Some(buf) = result {
        // SAFETY: `out` has room for the result.
        unsafe { buf.give(out) };
    }
    // SAFETY: `join` takes the handle (`own self`).
    unsafe { arc_drop::<TaskState>(handle) };
}

/// Releases a task handle that was not joined.
///
/// # Safety
///
/// `handle` must be a live task handle, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_drop(handle: *mut u8) {
    // SAFETY: forwarded.
    unsafe { arc_drop::<TaskState>(handle) };
}

/// Starts a detached task running the closure (which the runtime now owns).
///
/// # Safety
///
/// `code`/`env` must be a closure with no arguments returning `Unit`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_task_detach(code: Code0, env: *mut u8) {
    let closure = Closure { code, env };
    spawn_detached(Box::new(move || {
        let mut unit = 0u8;
        // SAFETY: the result is `Unit`.
        unsafe { closure.call_once(&raw mut unit) };
    }));
}

/// Lets other tasks run.
#[unsafe(no_mangle)]
pub extern "C" fn loom_task_yield() {
    yield_now();
}

// ----- channels ---------------------------------------------------------------------------

/// A channel of values described by `info`.
struct NativeChan {
    chan: Chan<Buf>,
    info: *const TypeInfo,
}

// SAFETY: the channel synchronizes access to its values; `info` is static data.
unsafe impl Send for NativeChan {}
// SAFETY: as above.
unsafe impl Sync for NativeChan {}

/// A new channel.
#[unsafe(no_mangle)]
pub extern "C" fn loom_chan_new(info: *const TypeInfo, capacity: i64) -> *mut u8 {
    arc_raw(NativeChan {
        chan: Chan::new(usize::try_from(capacity).unwrap_or(0)),
        info,
    })
}

/// Sends the value at `value` (moved in): 1 sent, 0 cancelled (the value is dropped).
/// Panics if the channel is closed.
///
/// # Safety
///
/// `ch` must be a live channel; `value` must hold a value of its element type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chan_send(
    ch: *mut u8,
    value: *const u8,
    file: *const u8,
    file_len: usize,
    line: u32,
    column: u32,
) -> u8 {
    // SAFETY: the caller passes a live channel.
    let ch = unsafe { arc_ref::<NativeChan>(ch) };
    // SAFETY: the value is moved into a buffer.
    let buf = unsafe { Buf::take(ch.info, value) };
    match ch.chan.send(buf) {
        Ok(()) => 1,
        Err(SendError::Cancelled(_)) => 0,
        Err(SendError::Closed(_)) => {
            // SAFETY: forwarded.
            let file = unsafe { crate::str_from(file, file_len) };
            crate::panic_at("send on a closed channel", file, line, column)
        }
    }
}

/// Receives a value into `out`: 1 received, 0 closed and empty (or cancelled).
///
/// # Safety
///
/// `ch` must be a live channel; `out` must have room for a value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chan_recv(ch: *mut u8, out: *mut u8, wait: u8) -> u8 {
    // SAFETY: the caller passes a live channel.
    let ch = unsafe { arc_ref::<NativeChan>(ch) };
    let value = if wait != 0 {
        ch.chan.recv()
    } else {
        ch.chan.try_recv()
    };
    match value {
        Some(buf) => {
            // SAFETY: `out` has room for the value.
            unsafe { buf.give(out) };
            1
        }
        None => 0,
    }
}

/// Closes a channel.
///
/// # Safety
///
/// `ch` must be a live channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chan_close(ch: *mut u8) {
    // SAFETY: the caller passes a live channel.
    unsafe { arc_ref::<NativeChan>(ch) }.chan.close();
}

/// Shares a channel handle.
///
/// # Safety
///
/// `ch` must be a live channel.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chan_clone(ch: *mut u8) -> *mut u8 {
    // SAFETY: forwarded.
    unsafe { arc_clone::<NativeChan>(ch) }
}

/// Releases a channel handle; the last one drops the values still in the channel.
///
/// # Safety
///
/// `ch` must be a live channel, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_chan_drop(ch: *mut u8) {
    // SAFETY: forwarded.
    unsafe { arc_drop::<NativeChan>(ch) };
}

/// One arm of a native `select`: kind 0 receive (into `data`, room for a value), 1 send
/// (the value at `data`, moved in), 2 timer (`nanos`).
#[repr(C)]
#[derive(Debug)]
pub struct SelectArm {
    /// The kind.
    pub kind: u32,
    /// The channel (receive and send arms).
    pub chan: *mut u8,
    /// The value buffer.
    pub data: *mut u8,
    /// The timer's duration.
    pub nanos: i64,
}

/// Waits on the arms of a `select` (spec §11.3). Returns the chosen arm, `count` for the
/// `else` arm, `count + 1` if no arm runs; `*received` is 1 if a receive arm got a value
/// (written to its buffer). Panics if a send arm finds its channel closed.
///
/// # Safety
///
/// `arms` must point to `count` arms whose channels are live and whose buffers are valid;
/// send arms' values are moved in.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_select(
    arms: *const SelectArm,
    count: usize,
    has_else: u8,
    received: *mut u8,
    file: *const u8,
    file_len: usize,
    line: u32,
    column: u32,
) -> i64 {
    // SAFETY: the caller passes `count` arms.
    let arms = unsafe { std::slice::from_raw_parts(arms, count) };
    let mut runtime_arms = Vec::with_capacity(count);
    for arm in arms {
        runtime_arms.push(match arm.kind {
            // SAFETY: receive arms carry live channels.
            0 => Arm::Recv(&unsafe { arc_ref::<NativeChan>(arm.chan) }.chan),
            1 => {
                // SAFETY: send arms carry live channels.
                let ch = unsafe { arc_ref::<NativeChan>(arm.chan) };
                // SAFETY: the value at `data` is moved in.
                let value = unsafe { Buf::take(ch.info, arm.data) };
                Arm::Send(&ch.chan, value)
            }
            _ => Arm::Timer(u64::try_from(arm.nanos).unwrap_or(0)),
        });
    }
    let index = |i: usize| i64::try_from(i).unwrap_or(i64::MAX);
    // SAFETY: `received` is writable.
    unsafe { *received = 0 };
    match select(runtime_arms, has_else != 0) {
        Selected::Recv(i, value) => {
            if let Some(buf) = value {
                // SAFETY: the arm's buffer has room for a value.
                unsafe {
                    buf.give(arms[i].data);
                    *received = 1;
                }
            }
            index(i)
        }
        Selected::Sent(i) | Selected::Timer(i) => index(i),
        Selected::Else => index(count),
        Selected::Cancelled => index(count + 1),
        Selected::SendClosed(_) => {
            // SAFETY: forwarded.
            let file = unsafe { crate::str_from(file, file_len) };
            crate::panic_at("send on a closed channel", file, line, column)
        }
    }
}

// ----- time ---------------------------------------------------------------------------------

/// Sleeps `nanos` nanoseconds: 1 if it slept, 0 if the task's scope was cancelled.
#[unsafe(no_mangle)]
pub extern "C" fn loom_sleep(nanos: i64) -> u8 {
    u8::from(sleep(u64::try_from(nanos).unwrap_or(0)))
}

/// The monotonic clock in nanoseconds.
#[unsafe(no_mangle)]
pub extern "C" fn loom_time_now() -> i64 {
    super::sched().map_or(0, |s| i64::try_from(s.now()).unwrap_or(i64::MAX))
}

// ----- mutexes, shared values, atomics -------------------------------------------------------

/// A mutex owning a value.
struct NativeMutex {
    lock: Lock,
    value: Buf,
}

// SAFETY: access to the value is serialized by the lock.
unsafe impl Send for NativeMutex {}
// SAFETY: as above.
unsafe impl Sync for NativeMutex {}

/// A new mutex owning the value at `value` (moved in).
///
/// # Safety
///
/// `value` must hold a value described by `info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mutex_new(info: *const TypeInfo, value: *const u8) -> *mut u8 {
    // SAFETY: the value is moved into a buffer.
    let value = unsafe { Buf::take(info, value) };
    arc_raw(NativeMutex {
        lock: Lock::new(),
        value,
    })
}

/// Runs the closure `code`/`env` with exclusive access to the mutex's value.
///
/// # Safety
///
/// `m` must be a live mutex; the closure takes a `mut` value and returns into `ret`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mutex_lock(m: *mut u8, code: Code1, env: *mut u8, ret: *mut u8) {
    // SAFETY: the caller passes a live mutex.
    let m = unsafe { arc_ref::<NativeMutex>(m) };
    m.lock.acquire();
    // SAFETY: the lock gives this task exclusive access to the value.
    unsafe { code(ret, env, m.value.ptr) };
    m.lock.release();
}

/// Shares a mutex handle.
///
/// # Safety
///
/// `m` must be a live mutex.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mutex_clone(m: *mut u8) -> *mut u8 {
    // SAFETY: forwarded.
    unsafe { arc_clone::<NativeMutex>(m) }
}

/// Releases a mutex handle.
///
/// # Safety
///
/// `m` must be a live mutex, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_mutex_drop(m: *mut u8) {
    // SAFETY: forwarded.
    unsafe { arc_drop::<NativeMutex>(m) };
}

/// A shared value.
struct NativeShared {
    value: Buf,
}

// SAFETY: the value is only read through shared handles (spec §8.5 rule 2).
unsafe impl Send for NativeShared {}
// SAFETY: as above.
unsafe impl Sync for NativeShared {}

/// The first owner of the value at `value` (moved in).
///
/// # Safety
///
/// `value` must hold a value described by `info`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_shared_new(info: *const TypeInfo, value: *const u8) -> *mut u8 {
    // SAFETY: the value is moved into a buffer.
    let value = unsafe { Buf::take(info, value) };
    arc_raw(NativeShared { value })
}

/// Another owner of a shared value.
///
/// # Safety
///
/// `s` must be a live shared handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_shared_clone(s: *mut u8) -> *mut u8 {
    // SAFETY: forwarded.
    unsafe { arc_clone::<NativeShared>(s) }
}

/// A pointer to the shared value.
///
/// # Safety
///
/// `s` must be a live shared handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_shared_get(s: *mut u8) -> *mut u8 {
    // SAFETY: the caller passes a live handle.
    unsafe { arc_ref::<NativeShared>(s) }.value.ptr
}

/// The number of owners.
///
/// # Safety
///
/// `s` must be a live shared handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_shared_count(s: *mut u8) -> i64 {
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let p = s.cast::<NativeShared>().cast_const();
    // SAFETY: the handle is live, so the count is at least one and stays valid.
    let arc = std::mem::ManuallyDrop::new(unsafe { Arc::from_raw(p) });
    i64::try_from(Arc::strong_count(&arc)).unwrap_or(i64::MAX)
}

/// Releases an owner; the last one drops the value.
///
/// # Safety
///
/// `s` must be a live shared handle, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_shared_drop(s: *mut u8) {
    // SAFETY: forwarded.
    unsafe { arc_drop::<NativeShared>(s) };
}

/// A new atomic integer.
#[unsafe(no_mangle)]
pub extern "C" fn loom_atomic_new(value: i64) -> *mut u8 {
    arc_raw(AtomicI64::new(value))
}

/// An operation on an atomic integer: 0 load, 1 store `a`, 2 add `a` (returns the old
/// value), 3 compare-exchange `a` → `b` (returns 1 on success).
///
/// # Safety
///
/// `h` must be a live atomic handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_atomic_op(h: *mut u8, op: i64, a: i64, b: i64) -> i64 {
    // SAFETY: the caller passes a live handle.
    let atomic = unsafe { arc_ref::<AtomicI64>(h) };
    match op {
        0 => atomic.load(Ordering::SeqCst),
        1 => {
            atomic.store(a, Ordering::SeqCst);
            0
        }
        2 => atomic.fetch_add(a, Ordering::SeqCst),
        _ => i64::from(
            atomic
                .compare_exchange(a, b, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
        ),
    }
}

/// Shares an atomic handle.
///
/// # Safety
///
/// `h` must be a live atomic handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_atomic_clone(h: *mut u8) -> *mut u8 {
    // SAFETY: forwarded.
    unsafe { arc_clone::<AtomicI64>(h) }
}

/// Releases an atomic handle.
///
/// # Safety
///
/// `h` must be a live atomic handle, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn loom_atomic_drop(h: *mut u8) {
    // SAFETY: forwarded.
    unsafe { arc_drop::<AtomicI64>(h) };
}

// ----- running native programs ------------------------------------------------------------------

/// Runs native `loom_main` (`entry`) as the first task of the scheduler (parallel unless
/// `LOOM_SCHED` says otherwise) and returns its exit status.
pub fn run_native_main(entry: extern "C" fn() -> i32) -> i32 {
    let status = Arc::new(Mutex::new(0));
    let out = Arc::clone(&status);
    let mode = super::Mode::from_env(super::Mode::Parallel {
        workers: super::default_workers(),
    });
    let stack = std::env::var("LOOM_TASK_STACK")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256 * 1024);
    super::run(mode, stack, 1 << 30, move || {
        *super::lock(&out) = entry();
    });
    *super::lock(&status)
}

/// The C entry of an executable: runs `loom_main` on the scheduler.
#[unsafe(no_mangle)]
pub extern "C" fn loom_run_main(entry: extern "C" fn() -> i32) -> i32 {
    let status = run_native_main(entry);
    crate::flush_stdout();
    status
}
