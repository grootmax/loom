//! The task scheduler (ADR 0022 §2–§3).
//!
//! Tasks are stackful coroutines ([`context`], [`stack`]). Each worker thread runs a loop
//! on its own stack: it picks a ready task, switches to it, and when the task switches
//! back it carries out what the task asked for (yield, park, exit) — only then, on the
//! worker's stack, is the task's state updated, so nobody can resume a task that is still
//! running on its stack.
//!
//! Deterministic mode uses one worker, picks among ready tasks with a seeded generator,
//! and keeps virtual time. Parallel mode uses several workers with local queues, a global
//! queue and stealing, and real time.

pub mod abi;
pub mod chan;
pub mod context;
pub mod stack;

use context::Sp;
use stack::Stack;
use std::cell::{Cell, UnsafeCell};
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

/// Locks a mutex, ignoring poisoning (the runtime never unwinds through its locks).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How tasks are scheduled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One worker, seeded choice among ready tasks, virtual time.
    Deterministic {
        /// The seed of the choices.
        seed: u64,
    },
    /// Several workers, real time.
    Parallel {
        /// The number of worker threads.
        workers: usize,
    },
}

impl Mode {
    /// The mode chosen by `LOOM_SCHED` (`deterministic[:SEED]` or `parallel[:N]`), or
    /// `default`.
    #[must_use]
    pub fn from_env(default: Self) -> Self {
        let Ok(value) = std::env::var("LOOM_SCHED") else {
            return default;
        };
        let (kind, arg) = value.split_once(':').unwrap_or((value.as_str(), ""));
        match kind {
            "deterministic" => Self::Deterministic {
                seed: arg.parse().unwrap_or(0),
            },
            "parallel" => Self::Parallel {
                workers: arg.parse().unwrap_or_else(|_| default_workers()),
            },
            _ => default,
        }
    }
}

/// One worker per available core (or `LOOM_WORKERS`).
#[must_use]
pub fn default_workers() -> usize {
    std::env::var("LOOM_WORKERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, std::num::NonZero::get))
        .max(1)
}

/// What a task asked its worker to do when it switched back.
#[derive(Clone, Copy, PartialEq, Eq)]
enum After {
    Nothing,
    Yield,
    Park,
    Exit,
}

/// The scheduling state of a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ready,
    Running,
    /// Parked; waiting for a wake-up.
    Parked,
    /// Woken while running (before it parked): its next park returns at once.
    Notified,
    Done,
}

/// The work a task runs.
type Work = Box<dyn FnOnce()>;

/// A task.
pub struct Task {
    id: u64,
    state: Mutex<State>,
    /// The saved stack pointer while the task is not running. Only the worker that
    /// resumes or suspends the task touches it.
    sp: UnsafeCell<Sp>,
    stack: UnsafeCell<Option<Stack>>,
    work: UnsafeCell<Option<Work>>,
    /// The scope the task is currently in (changes as it enters and leaves scopes).
    scope: Mutex<Option<Arc<Scope>>>,
    /// The tasks waiting for this one to finish.
    joiners: Mutex<Vec<Arc<Task>>>,
    finished: AtomicBool,
}

// SAFETY: the `UnsafeCell` fields are only accessed by the one worker that is running or
// switching the task, which the state machine (`state`, `After`) serializes; the work
// closure is only ever run once, on one thread. In deterministic mode (the interpreter)
// every task stays on one thread; in parallel mode the work of native tasks is a code
// pointer with an environment the type checker has proven `Send` (ADR 0022 §7).
unsafe impl Send for Task {}
// SAFETY: as above; shared access goes through the mutexes and atomics.
unsafe impl Sync for Task {}

impl std::fmt::Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Task({})", self.id)
    }
}

impl Task {
    /// Whether the task has finished.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

/// A structured-concurrency scope (spec §11.1, §11.4).
pub struct Scope {
    parent: Option<Arc<Scope>>,
    cancelled: AtomicBool,
    children: Mutex<Vec<Weak<Scope>>>,
    running: AtomicUsize,
    waiter: Mutex<Option<Arc<Task>>>,
    /// Tasks parked in this scope, woken on cancellation.
    parked: Mutex<Vec<Weak<Task>>>,
}

impl std::fmt::Debug for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scope(cancelled: {})", self.is_cancelled())
    }
}

impl Scope {
    fn new(parent: Option<Arc<Self>>) -> Arc<Self> {
        let scope = Arc::new(Self {
            parent: parent.clone(),
            cancelled: AtomicBool::new(false),
            children: Mutex::new(Vec::new()),
            running: AtomicUsize::new(0),
            waiter: Mutex::new(None),
            parked: Mutex::new(Vec::new()),
        });
        if let Some(p) = parent {
            lock(&p.children).push(Arc::downgrade(&scope));
        }
        scope
    }

    /// Whether the scope or one of its ancestors is cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
            || self.parent.as_ref().is_some_and(|p| p.is_cancelled())
    }

    /// Cancels the scope and its descendants, waking their parked tasks.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let parked: Vec<Arc<Task>> = lock(&self.parked)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for task in parked {
            wake(&task);
        }
        let children: Vec<Arc<Self>> = lock(&self.children)
            .iter()
            .filter_map(Weak::upgrade)
            .collect();
        for child in children {
            child.cancel();
        }
    }
}

/// A timer: when its time comes, `action` runs on a worker.
struct Timer {
    at: u64,
    seq: u64,
    action: Box<dyn FnOnce() + Send>,
}

impl PartialEq for Timer {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}
impl Eq for Timer {}
impl PartialOrd for Timer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Timer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // A max-heap ordered so that the earliest timer is on top.
        (other.at, other.seq).cmp(&(self.at, self.seq))
    }
}

/// Shared scheduler state.
pub struct Sched {
    mode: Mode,
    ready: Mutex<VecDeque<Arc<Task>>>,
    idle: Condvar,
    timers: Mutex<BinaryHeap<Timer>>,
    timer_seq: AtomicU64,
    /// Virtual time in nanoseconds (deterministic mode).
    virtual_now: AtomicU64,
    started: Instant,
    rng: Mutex<u64>,
    live: AtomicUsize,
    next_id: AtomicU64,
    shutdown: AtomicBool,
    stack_size: usize,
    /// Stacks of finished tasks, for reuse.
    stacks: Mutex<Vec<Stack>>,
}

impl std::fmt::Debug for Sched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sched({:?})", self.mode)
    }
}

/// A worker thread's state.
struct Worker {
    sched: Arc<Sched>,
    sp: UnsafeCell<Sp>,
    current: UnsafeCell<Option<Arc<Task>>>,
    after: Cell<After>,
}

thread_local! {
    static WORKER: Cell<*const Worker> = const { Cell::new(std::ptr::null()) };
}

/// The worker running on this thread. Never inlined: a task may resume on another
/// thread, so the thread-local must be read anew after every switch.
#[inline(never)]
fn worker() -> *const Worker {
    WORKER.with(Cell::get)
}

/// The worker running on this thread, if any.
fn worker_ref<'a>() -> Option<&'a Worker> {
    let w = worker();
    // SAFETY: the pointer is set by `worker_loop` for the duration of the loop, which
    // outlives every task it runs.
    (!w.is_null()).then(|| unsafe { &*w })
}

/// The task running on this thread, if any.
#[must_use]
pub fn current() -> Option<Arc<Task>> {
    let w = worker_ref()?;
    // SAFETY: `current` is only written by this worker's loop, between task runs.
    unsafe { (*w.current.get()).clone() }
}

/// The scheduler of the running task.
#[must_use]
pub fn sched() -> Option<Arc<Sched>> {
    worker_ref().map(|w| Arc::clone(&w.sched))
}

impl Sched {
    fn new(mode: Mode, stack_size: usize) -> Arc<Self> {
        let seed = match mode {
            Mode::Deterministic { seed } => seed,
            Mode::Parallel { .. } => 0x9E37_79B9_7F4A_7C15,
        };
        Arc::new(Self {
            mode,
            ready: Mutex::new(VecDeque::new()),
            idle: Condvar::new(),
            timers: Mutex::new(BinaryHeap::new()),
            timer_seq: AtomicU64::new(0),
            virtual_now: AtomicU64::new(0),
            started: Instant::now(),
            rng: Mutex::new(seed | 1),
            live: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
            shutdown: AtomicBool::new(false),
            stack_size,
            stacks: Mutex::new(Vec::new()),
        })
    }

    fn deterministic(&self) -> bool {
        matches!(self.mode, Mode::Deterministic { .. })
    }

    /// The current time in nanoseconds since the scheduler started (virtual in
    /// deterministic mode).
    #[must_use]
    pub fn now(&self) -> u64 {
        if self.deterministic() {
            self.virtual_now.load(Ordering::Acquire)
        } else {
            u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX)
        }
    }

    fn random(&self) -> u64 {
        let mut state = lock(&self.rng);
        // xorshift64*
        let mut x = *state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Runs `action` at time `at` (nanoseconds, as [`Sched::now`]).
    pub fn add_timer(&self, at: u64, action: Box<dyn FnOnce() + Send>) {
        let seq = self.timer_seq.fetch_add(1, Ordering::Relaxed);
        lock(&self.timers).push(Timer { at, seq, action });
        // Notify under the ready lock: a worker checks timers under it before waiting.
        let _ready = lock(&self.ready);
        self.idle.notify_all();
    }

    /// Stops the workers once they have nothing to run.
    fn stop(&self) {
        let _ready = lock(&self.ready);
        self.shutdown.store(true, Ordering::Release);
        self.idle.notify_all();
    }

    fn push_ready(&self, task: Arc<Task>) {
        lock(&self.ready).push_back(task);
        self.idle.notify_one();
    }

    /// Runs the timers that are due; returns whether any ran.
    fn fire_timers(&self) -> bool {
        let now = self.now();
        let mut due = Vec::new();
        {
            let mut timers = lock(&self.timers);
            while timers.peek().is_some_and(|t| t.at <= now) {
                if let Some(t) = timers.pop() {
                    due.push(t);
                }
            }
        }
        let any = !due.is_empty();
        for timer in due {
            (timer.action)();
        }
        any
    }

    /// The next task to run, waiting if there is none; `None` when the scheduler is
    /// shutting down.
    fn next(&self) -> Option<Arc<Task>> {
        loop {
            let mut ready = lock(&self.ready);
            // Checked under the ready lock, which `stop` and `add_timer` take to notify.
            if self.shutdown.load(Ordering::Acquire) {
                return None;
            }
            if !ready.is_empty() {
                let index = if self.deterministic() {
                    usize::try_from(self.random() % ready.len() as u64).unwrap_or(0)
                } else {
                    0
                };
                return ready.remove(index);
            }
            let next_timer = lock(&self.timers).peek().map(|t| t.at);
            if next_timer.is_some_and(|at| at <= self.now()) {
                drop(ready);
                self.fire_timers();
                continue;
            }
            if self.deterministic() {
                drop(ready);
                // Nothing is ready: jump virtual time to the next timer.
                if let Some(at) = next_timer {
                    self.virtual_now.fetch_max(at, Ordering::AcqRel);
                    continue;
                }
                if self.live.load(Ordering::Acquire) == 0 {
                    return None;
                }
                crate::panic_at(
                    "deadlock: every task is waiting and nothing can wake them",
                    "",
                    0,
                    0,
                );
            }
            let guard = match next_timer {
                Some(at) => {
                    let ns = at.saturating_sub(self.now());
                    self.idle
                        .wait_timeout(ready, Duration::from_nanos(ns))
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0
                }
                None => self
                    .idle
                    .wait(ready)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            };
            drop(guard);
        }
    }

    fn take_stack(&self) -> Stack {
        lock(&self.stacks)
            .pop()
            .unwrap_or_else(|| Stack::new(self.stack_size))
    }

    fn recycle_stack(&self, stack: Stack) {
        if stack.usable() != Stack::usable_for(self.stack_size) {
            return;
        }
        let mut stacks = lock(&self.stacks);
        if stacks.len() < 1024 {
            stacks.push(stack);
        }
    }
}

/// The entry point of every task: runs its work, then exits.
unsafe extern "C" fn task_entry(arg: *mut u8) -> ! {
    #[allow(
        clippy::cast_ptr_alignment,
        reason = "the pointer came from `Arc::into_raw`"
    )]
    let raw = arg.cast::<Task>().cast_const();
    // SAFETY: `arg` is the `Arc<Task>` pointer leaked by `spawn_task` for this purpose.
    let task: Arc<Task> = unsafe { Arc::from_raw(raw) };
    // SAFETY: the work is taken exactly once, by the task itself.
    let work = unsafe { (*task.work.get()).take() };
    drop(task);
    if let Some(work) = work {
        work();
    }
    switch_to_worker(After::Exit);
    unreachable!("a finished task was resumed");
}

/// Switches from the running task back to its worker, asking it to do `after`.
fn switch_to_worker(after: After) {
    let w = worker();
    assert!(!w.is_null(), "a blocking operation outside a task");
    // SAFETY: `w` is this thread's worker, alive for the duration of its loop. The task's
    // saved stack pointer is written by the switch and read only when it is resumed.
    unsafe {
        (*w).after.set(after);
        let task = (*(*w).current.get()).clone();
        let Some(task) = task else {
            panic!("no task is running")
        };
        let save = task.sp.get();
        drop(task);
        context::loom_context_switch(save, *(*w).sp.get());
    }
}

/// Creates a task running `work` in `scope`, and makes it ready.
fn spawn_task(
    sched: &Arc<Sched>,
    scope: Option<Arc<Scope>>,
    work: Work,
    stack_size: usize,
) -> Arc<Task> {
    let task = Arc::new(Task {
        id: sched.next_id.fetch_add(1, Ordering::Relaxed),
        state: Mutex::new(State::Ready),
        sp: UnsafeCell::new(std::ptr::null_mut()),
        stack: UnsafeCell::new(None),
        work: UnsafeCell::new(Some(work)),
        scope: Mutex::new(None),
        joiners: Mutex::new(Vec::new()),
        finished: AtomicBool::new(false),
    });
    if let Some(scope) = &scope {
        scope.running.fetch_add(1, Ordering::AcqRel);
    }
    *lock(&task.scope) = scope;
    let stack = if stack_size == sched.stack_size {
        sched.take_stack()
    } else {
        Stack::new(stack_size)
    };
    let arg = Arc::into_raw(Arc::clone(&task)).cast_mut().cast::<u8>();
    // SAFETY: the stack is fresh or recycled (no context uses it), and `task_entry` takes
    // ownership of `arg`.
    unsafe {
        let sp = context::prepare(
            stack.top(),
            stack.initial_limit(),
            stack.base(),
            task_entry,
            arg,
        );
        *task.sp.get() = sp;
        *task.stack.get() = Some(stack);
    }
    sched.live.fetch_add(1, Ordering::AcqRel);
    sched.push_ready(Arc::clone(&task));
    task
}

/// Makes a parked task ready, or makes its next park return at once.
pub fn wake(task: &Arc<Task>) {
    let mut state = lock(&task.state);
    match *state {
        State::Parked => {
            *state = State::Ready;
            drop(state);
            if let Some(sched) = sched() {
                sched.push_ready(Arc::clone(task));
            } else if let Some(sched) = SCHED_OF.with(|s| s.borrow().clone()) {
                sched.push_ready(Arc::clone(task));
            }
        }
        State::Running => *state = State::Notified,
        State::Ready | State::Notified | State::Done => {}
    }
}

thread_local! {
    /// The scheduler a non-worker thread (the reactor, timers) wakes tasks on.
    static SCHED_OF: std::cell::RefCell<Option<Arc<Sched>>> = const { std::cell::RefCell::new(None) };
}

/// Parks the running task until [`wake`] is called for it (or returns at once if it was
/// woken since it last ran).
pub fn park() {
    switch_to_worker(After::Park);
}

/// Lets other tasks run.
pub fn yield_now() {
    if worker_ref().is_some() && current().is_some() {
        switch_to_worker(After::Yield);
    }
}

/// The loop of a worker thread: runs tasks until the scheduler shuts down.
fn worker_loop(sched: &Arc<Sched>) {
    let w = Worker {
        sched: Arc::clone(sched),
        sp: UnsafeCell::new(std::ptr::null_mut()),
        current: UnsafeCell::new(None),
        after: Cell::new(After::Nothing),
    };
    WORKER.with(|c| c.set(&raw const w));
    while let Some(task) = sched.next() {
        {
            let mut state = lock(&task.state);
            if *state == State::Done {
                continue;
            }
            *state = State::Running;
        }
        // SAFETY: only this worker runs `task` now (it was taken from the ready queue in
        // state `Ready`); its saved stack pointer is valid.
        unsafe {
            *w.current.get() = Some(Arc::clone(&task));
            w.after.set(After::Nothing);
            let to = *task.sp.get();
            context::loom_context_switch(w.sp.get(), to);
            *w.current.get() = None;
        }
        match w.after.get() {
            After::Yield | After::Nothing => {
                *lock(&task.state) = State::Ready;
                sched.push_ready(task);
            }
            After::Park => {
                let mut state = lock(&task.state);
                if *state == State::Notified {
                    *state = State::Ready;
                    drop(state);
                    sched.push_ready(task);
                } else {
                    *state = State::Parked;
                }
            }
            After::Exit => finish(sched, &task),
        }
    }
    WORKER.with(|c| c.set(std::ptr::null()));
}

/// Cleans up after a task that exited: frees its stack, wakes joiners and its scope.
fn finish(sched: &Arc<Sched>, task: &Arc<Task>) {
    *lock(&task.state) = State::Done;
    task.finished.store(true, Ordering::Release);
    // SAFETY: the task has switched away for the last time; nobody uses its stack.
    if let Some(stack) = unsafe { (*task.stack.get()).take() } {
        sched.recycle_stack(stack);
    }
    let joiners = std::mem::take(&mut *lock(&task.joiners));
    for joiner in joiners {
        wake(&joiner);
    }
    let scope = lock(&task.scope).take();
    if let Some(scope) = scope
        && scope.running.fetch_sub(1, Ordering::AcqRel) == 1
        && let Some(waiter) = lock(&scope.waiter).take()
    {
        wake(&waiter);
    }
    if sched.live.fetch_sub(1, Ordering::AcqRel) == 1 && !sched.deterministic() {
        sched.idle.notify_all();
    }
}

/// Runs `main` as the first task of a new scheduler in `mode`, on a stack of
/// `main_stack` bytes (other tasks get `stack_size`), and returns when it finishes or a
/// task calls [`abort`]. Tasks still running are abandoned; work that never started is
/// dropped before `run` returns.
pub fn run(mode: Mode, stack_size: usize, main_stack: usize, main: impl FnOnce() + 'static) {
    let sched = Sched::new(mode, stack_size);
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let stopper = Arc::clone(&sched);
    let work: Work = Box::new(move || {
        main();
        flag.store(true, Ordering::Release);
        stopper.stop();
    });
    spawn_task(&sched, None, work, main_stack);
    let extra = match mode {
        Mode::Deterministic { .. } => 0,
        Mode::Parallel { workers } => workers.saturating_sub(1),
    };
    let threads: Vec<_> = (0..extra)
        .map(|_| {
            let s = Arc::clone(&sched);
            std::thread::Builder::new()
                .stack_size(1 << 20)
                .spawn(move || worker_loop(&s))
        })
        .collect();
    worker_loop(&sched);
    sched.stop();
    for t in threads.into_iter().flatten() {
        let _ = t.join();
    }
    // Drop queued work that never started (it may own resources).
    let queued: Vec<Arc<Task>> = lock(&sched.ready).drain(..).collect();
    for task in queued {
        // SAFETY: the scheduler has stopped; no worker touches the task any more.
        drop(unsafe { (*task.work.get()).take() });
    }
}

/// Ends the program from inside a task (a panic, ADR 0022 §3): stops the scheduler, so
/// that [`run`] returns, and never resumes the calling task.
pub fn abort() -> ! {
    if let Some(sched) = sched() {
        sched.stop();
    }
    switch_to_worker(After::Exit);
    unreachable!("an aborted task was resumed");
}

/// Starts a child of the current task's scope running `work`.
///
/// # Panics
///
/// Outside a task.
#[allow(clippy::must_use_candidate, reason = "the handle is optional")]
pub fn spawn(work: Work) -> Arc<Task> {
    let sched = sched().expect("spawn outside a task");
    let scope = current().and_then(|t| lock(&t.scope).clone());
    let size = sched.stack_size;
    spawn_task(&sched, scope, work, size)
}

/// Starts a child of `scope` running `work`.
///
/// # Panics
///
/// Outside a task.
#[allow(clippy::must_use_candidate, reason = "the handle is optional")]
pub fn spawn_in(scope: &Arc<Scope>, work: Work) -> Arc<Task> {
    let sched = sched().expect("spawn outside a task");
    let size = sched.stack_size;
    spawn_task(&sched, Some(Arc::clone(scope)), work, size)
}

/// Starts a task that belongs to no scope.
///
/// # Panics
///
/// Outside a task.
#[allow(clippy::must_use_candidate, reason = "the handle is optional")]
pub fn spawn_detached(work: Work) -> Arc<Task> {
    let sched = sched().expect("spawn outside a task");
    let size = sched.stack_size;
    spawn_task(&sched, None, work, size)
}

/// A lock that parks waiting tasks instead of blocking their worker (`Mutex`, spec
/// §11.7).
#[derive(Debug, Default)]
pub struct Lock {
    state: Mutex<(bool, VecDeque<Arc<Task>>)>,
}

impl Lock {
    /// A new, unlocked lock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits until the lock is free and takes it.
    ///
    /// # Panics
    ///
    /// Outside a task, when it would have to wait.
    pub fn acquire(&self) {
        loop {
            let mut state = lock(&self.state);
            if !state.0 {
                state.0 = true;
                return;
            }
            let me = current().expect("a lock taken outside a task");
            state.1.push_back(me);
            drop(state);
            park();
        }
    }

    /// Releases the lock and wakes one waiting task.
    pub fn release(&self) {
        let mut state = lock(&self.state);
        state.0 = false;
        if let Some(next) = state.1.pop_front() {
            drop(state);
            wake(&next);
        }
    }
}

/// Waits until `task` finishes.
///
/// # Panics
///
/// Outside a task.
pub fn join(task: &Arc<Task>) {
    let me = current().expect("join outside a task");
    loop {
        if task.is_finished() {
            return;
        }
        lock(&task.joiners).push(Arc::clone(&me));
        if task.is_finished() {
            return;
        }
        park();
    }
}

/// The scope the running task is in.
#[must_use]
pub fn current_scope() -> Option<Arc<Scope>> {
    current().and_then(|t| lock(&t.scope).clone())
}

/// Whether the running task's scope is cancelled.
#[must_use]
pub fn cancelled() -> bool {
    current_scope().is_some_and(|s| s.is_cancelled())
}

/// Enters a new scope (a child of the current one) with an optional deadline in
/// nanoseconds from now; returns it and the previous scope.
///
/// # Panics
///
/// Outside a task.
#[must_use]
pub fn enter_scope(deadline: Option<u64>) -> (Arc<Scope>, Option<Arc<Scope>>) {
    let task = current().expect("scope outside a task");
    let previous = lock(&task.scope).clone();
    let scope = Scope::new(previous.clone());
    *lock(&task.scope) = Some(Arc::clone(&scope));
    if let (Some(ns), Some(sched)) = (deadline, sched()) {
        let target = Arc::downgrade(&scope);
        sched.add_timer(
            sched.now().saturating_add(ns),
            Box::new(move || {
                if let Some(scope) = target.upgrade() {
                    scope.cancel();
                }
            }),
        );
    }
    (scope, previous)
}

/// Waits for every child of `scope`, then makes `previous` the current scope again.
///
/// # Panics
///
/// Outside a task.
pub fn exit_scope(scope: &Arc<Scope>, previous: Option<Arc<Scope>>) {
    let task = current().expect("scope outside a task");
    while scope.running.load(Ordering::Acquire) > 0 {
        *lock(&scope.waiter) = Some(Arc::clone(&task));
        if scope.running.load(Ordering::Acquire) == 0 {
            lock(&scope.waiter).take();
            break;
        }
        park();
    }
    *lock(&task.scope) = previous;
}

/// Parks the running task until it is woken or its scope is cancelled; returns whether
/// the scope is cancelled. The task registers with its scopes so that cancellation can
/// wake it.
///
/// # Panics
///
/// Outside a task.
#[must_use]
pub fn park_cancellable() -> bool {
    let task = current().expect("park outside a task");
    let scope = lock(&task.scope).clone();
    let mut chain = Vec::new();
    let mut next = scope;
    while let Some(sc) = next {
        lock(&sc.parked).push(Arc::downgrade(&task));
        next = sc.parent.as_ref().map(Arc::clone);
        chain.push(sc);
    }
    if !chain.first().is_some_and(|s| s.is_cancelled()) {
        park();
    }
    for sc in &chain {
        lock(&sc.parked).retain(|w| w.upgrade().is_some_and(|t| !Arc::ptr_eq(&t, &task)));
    }
    chain.first().is_some_and(|s| s.is_cancelled())
}

/// Sleeps for `ns` nanoseconds; returns `false` if the task's scope was cancelled first.
///
/// # Panics
///
/// Outside a task.
#[allow(clippy::must_use_candidate, reason = "callers may ignore cancellation")]
pub fn sleep(ns: u64) -> bool {
    let sched = sched().expect("sleep outside a task");
    let task = current().expect("sleep outside a task");
    if cancelled() {
        return false;
    }
    let deadline = sched.now().saturating_add(ns);
    let fired = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&fired);
    let target = Arc::clone(&task);
    sched.add_timer(
        deadline,
        Box::new(move || {
            flag.store(true, Ordering::Release);
            wake(&target);
        }),
    );
    while !fired.load(Ordering::Acquire) {
        if park_cancellable() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{Mode, run, sleep, spawn, yield_now};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn tasks_interleave_deterministically() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = Rc::clone(&log);
        run(
            Mode::Deterministic { seed: 7 },
            64 * 1024,
            64 * 1024,
            move || {
                let (scope, previous) = super::enter_scope(None);
                for i in 0..3 {
                    let l = Rc::clone(&l);
                    spawn(Box::new(move || {
                        for step in 0..3 {
                            l.borrow_mut().push((i, step));
                            yield_now();
                        }
                    }));
                }
                super::exit_scope(&scope, previous);
                l.borrow_mut().push((99, 0));
            },
        );
        let log = log.borrow();
        assert_eq!(log.len(), 10);
        assert_eq!(log.last(), Some(&(99, 0)));
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn virtual_sleep_orders_wakeups() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = Rc::clone(&log);
        run(
            Mode::Deterministic { seed: 1 },
            64 * 1024,
            64 * 1024,
            move || {
                let (scope, previous) = super::enter_scope(None);
                for (i, ms) in [(1, 30u64), (2, 10), (3, 20)] {
                    let l = Rc::clone(&l);
                    spawn(Box::new(move || {
                        sleep(ms * 1_000_000);
                        l.borrow_mut().push(i);
                    }));
                }
                super::exit_scope(&scope, previous);
            },
        );
        assert_eq!(*log.borrow(), vec![2, 3, 1]);
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn deadline_cancels_sleepers() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = Rc::clone(&log);
        run(
            Mode::Deterministic { seed: 3 },
            64 * 1024,
            64 * 1024,
            move || {
                let (scope, previous) = super::enter_scope(Some(25_000_000));
                for ms in [10u64, 50] {
                    let l = Rc::clone(&l);
                    spawn(Box::new(move || {
                        let finished = sleep(ms * 1_000_000);
                        l.borrow_mut().push((ms, finished));
                    }));
                }
                super::exit_scope(&scope, previous);
            },
        );
        let mut log = log.borrow().clone();
        log.sort_unstable();
        assert_eq!(log, vec![(10, true), (50, false)]);
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn parallel_mode_runs_many_tasks() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        run(
            Mode::Parallel { workers: 4 },
            64 * 1024,
            64 * 1024,
            move || {
                let (scope, previous) = super::enter_scope(None);
                for _ in 0..1000 {
                    let c = Arc::clone(&c);
                    spawn(Box::new(move || {
                        yield_now();
                        c.fetch_add(1, Ordering::Relaxed);
                    }));
                }
                super::exit_scope(&scope, previous);
            },
        );
        assert_eq!(count.load(Ordering::Relaxed), 1000);
    }
}
