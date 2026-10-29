//! Channels and `select` (spec §11.2–§11.3, ADR 0022 §4).
//!
//! A waiting operation is a waiter queued on a channel. Every waiter has a token
//! shared by the operations of one `select` (a plain `send`/`recv` has its own): an
//! operation completes a waiter only after claiming its token, so exactly one arm of a
//! `select` fires.

use super::{Task, current, lock, park_cancellable, sched, wake};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// The token value of an operation abandoned because its scope was cancelled.
const CANCELLED: usize = usize::MAX;

/// A waiting `send` or `recv`.
struct Waiter<T> {
    task: Arc<Task>,
    token: Arc<AtomicUsize>,
    arm: usize,
    /// The value being sent, or the value received.
    slot: Mutex<Option<T>>,
    done: AtomicBool,
    /// The channel was closed while the operation waited.
    closed: AtomicBool,
}

impl<T> Waiter<T> {
    fn new(task: Arc<Task>, token: Arc<AtomicUsize>, arm: usize, value: Option<T>) -> Arc<Self> {
        Arc::new(Self {
            task,
            token,
            arm,
            slot: Mutex::new(value),
            done: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }

    /// Claims the waiter's token for its arm; fails if another arm (or cancellation) won.
    fn claim(&self) -> bool {
        self.token
            .compare_exchange(0, self.arm + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn complete(&self) {
        self.done.store(true, Ordering::Release);
        wake(&self.task);
    }
}

struct Inner<T> {
    buffer: VecDeque<T>,
    closed: bool,
    receivers: VecDeque<Arc<Waiter<T>>>,
    senders: VecDeque<Arc<Waiter<T>>>,
}

/// A channel of values of type `T`.
pub struct Chan<T> {
    inner: Mutex<Inner<T>>,
    capacity: usize,
}

impl<T> std::fmt::Debug for Chan<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Chan(capacity: {})", self.capacity)
    }
}

/// Why a `send` did not complete.
#[derive(Debug)]
pub enum SendError<T> {
    /// The channel is closed (a panic in Loom, spec §11.2 rule 2).
    Closed(T),
    /// The task's scope was cancelled; the value is returned to be dropped.
    Cancelled(T),
}

/// The outcome of a `select`.
#[derive(Debug)]
pub enum Selected<T> {
    /// Receive arm `arm` completed with this result (`None`: closed or cancelled).
    Recv(usize, Option<T>),
    /// Send arm `arm` completed.
    Sent(usize),
    /// Timer arm `arm` fired.
    Timer(usize),
    /// No arm could proceed and the `select` has an `else` arm.
    Else,
    /// The scope was cancelled and the `select` has no receive arm (spec §11.3 rule 5).
    Cancelled,
    /// Send arm `arm` hit a closed channel (a panic in Loom).
    SendClosed(usize),
}

/// One arm of a `select`.
pub enum Arm<'a, T> {
    /// Receive from a channel.
    Recv(&'a Chan<T>),
    /// Send a value (evaluated when the `select` started) on a channel.
    Send(&'a Chan<T>, T),
    /// Wait this many nanoseconds from the start of the `select`.
    Timer(u64),
}

impl<T> Chan<T> {
    /// A channel holding up to `capacity` values (0: rendezvous).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                buffer: VecDeque::new(),
                closed: false,
                receivers: VecDeque::new(),
                senders: VecDeque::new(),
            }),
            capacity,
        }
    }

    /// Hands `value` to a waiting receiver or buffers it; gives it back if neither is
    /// possible now.
    fn try_send_locked(&self, inner: &mut Inner<T>, value: T) -> Result<(), T> {
        while let Some(r) = inner.receivers.pop_front() {
            if r.claim() {
                *lock(&r.slot) = Some(value);
                r.complete();
                return Ok(());
            }
        }
        if inner.buffer.len() < self.capacity {
            inner.buffer.push_back(value);
            return Ok(());
        }
        Err(value)
    }

    /// Takes a value from the buffer or a waiting sender, if there is one.
    fn try_recv_locked(inner: &mut Inner<T>) -> Option<T> {
        if let Some(v) = inner.buffer.pop_front() {
            while let Some(s) = inner.senders.pop_front() {
                if s.claim() {
                    if let Some(sv) = lock(&s.slot).take() {
                        inner.buffer.push_back(sv);
                    }
                    s.complete();
                    break;
                }
            }
            return Some(v);
        }
        while let Some(s) = inner.senders.pop_front() {
            if s.claim() {
                let v = lock(&s.slot).take();
                s.complete();
                return v;
            }
        }
        None
    }

    /// Sends `value`, waiting while the channel is full.
    ///
    /// # Errors
    ///
    /// [`SendError::Closed`] if the channel is (or becomes) closed, and
    /// [`SendError::Cancelled`] if the task's scope is cancelled first.
    ///
    /// # Panics
    ///
    /// Outside a task, when it would have to wait.
    pub fn send(&self, value: T) -> Result<(), SendError<T>> {
        let mut inner = lock(&self.inner);
        if inner.closed {
            return Err(SendError::Closed(value));
        }
        let value = match self.try_send_locked(&mut inner, value) {
            Ok(()) => return Ok(()),
            Err(v) => v,
        };
        if super::cancelled() {
            return Err(SendError::Cancelled(value));
        }
        let task = current().expect("a channel operation outside a task");
        let waiter = Waiter::new(task, Arc::new(AtomicUsize::new(0)), 0, Some(value));
        inner.senders.push_back(Arc::clone(&waiter));
        drop(inner);
        loop {
            if waiter.done.load(Ordering::Acquire) {
                return if waiter.closed.load(Ordering::Acquire) {
                    let v = lock(&waiter.slot).take();
                    v.map_or(Ok(()), |v| Err(SendError::Closed(v)))
                } else {
                    Ok(())
                };
            }
            if park_cancellable() && self.abandon(&waiter, true) {
                let v = lock(&waiter.slot).take();
                return v.map_or(Ok(()), |v| Err(SendError::Cancelled(v)));
            }
        }
    }

    /// Withdraws a waiting operation after cancellation; fails if it completed meanwhile.
    fn abandon(&self, waiter: &Arc<Waiter<T>>, sender: bool) -> bool {
        if waiter
            .token
            .compare_exchange(0, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let mut inner = lock(&self.inner);
        let queue = if sender {
            &mut inner.senders
        } else {
            &mut inner.receivers
        };
        queue.retain(|w| !Arc::ptr_eq(w, waiter));
        true
    }

    /// Receives the oldest value, waiting while the channel is empty and open; `None` once
    /// it is closed and empty, or if the task's scope is cancelled.
    ///
    /// # Panics
    ///
    /// Outside a task, when it would have to wait.
    pub fn recv(&self) -> Option<T> {
        let mut inner = lock(&self.inner);
        if let Some(v) = Self::try_recv_locked(&mut inner) {
            return Some(v);
        }
        if inner.closed || super::cancelled() {
            return None;
        }
        let task = current().expect("a channel operation outside a task");
        let waiter = Waiter::new(task, Arc::new(AtomicUsize::new(0)), 0, None);
        inner.receivers.push_back(Arc::clone(&waiter));
        drop(inner);
        loop {
            if waiter.done.load(Ordering::Acquire) {
                return lock(&waiter.slot).take();
            }
            if park_cancellable() && self.abandon(&waiter, false) {
                return None;
            }
        }
    }

    /// Receives a value if one is available now.
    pub fn try_recv(&self) -> Option<T> {
        let mut inner = lock(&self.inner);
        Self::try_recv_locked(&mut inner)
    }

    /// Closes the channel: waiting receivers get `None`, waiting senders fail.
    pub fn close(&self) {
        let mut inner = lock(&self.inner);
        if inner.closed {
            return;
        }
        inner.closed = true;
        for r in std::mem::take(&mut inner.receivers) {
            if r.claim() {
                r.closed.store(true, Ordering::Release);
                r.complete();
            }
        }
        for s in std::mem::take(&mut inner.senders) {
            if s.claim() {
                s.closed.store(true, Ordering::Release);
                s.complete();
            }
        }
    }

    /// Whether the channel is closed.
    pub fn is_closed(&self) -> bool {
        lock(&self.inner).closed
    }
}

/// Waits until one arm can proceed (spec §11.3). Arms are tried in a random order; with
/// `has_else`, never waits.
///
/// # Panics
///
/// Outside a task, when it would have to wait.
#[allow(clippy::too_many_lines, reason = "the select protocol in one place")]
pub fn select<T>(arms: Vec<Arm<'_, T>>, has_else: bool) -> Selected<T> {
    let sched = sched().expect("select outside a task");
    let n = arms.len();
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = usize::try_from(sched.random() % (i as u64 + 1)).unwrap_or(0);
        order.swap(i, j);
    }
    let is_recv: Vec<bool> = arms.iter().map(|a| matches!(a, Arm::Recv(_))).collect();
    let chans: Vec<Option<&Chan<T>>> = arms
        .iter()
        .map(|a| match a {
            Arm::Recv(ch) | Arm::Send(ch, _) => Some(*ch),
            Arm::Timer(_) => None,
        })
        .collect();
    let mut arms: Vec<Option<Arm<'_, T>>> = arms.into_iter().map(Some).collect();
    // First pass: an arm that can proceed now.
    for &i in &order {
        match arms[i].take() {
            Some(Arm::Recv(ch)) => {
                let mut inner = lock(&ch.inner);
                if let Some(v) = Chan::try_recv_locked(&mut inner) {
                    return Selected::Recv(i, Some(v));
                }
                if inner.closed {
                    return Selected::Recv(i, None);
                }
                drop(inner);
                arms[i] = Some(Arm::Recv(ch));
            }
            Some(Arm::Send(ch, value)) => {
                let mut inner = lock(&ch.inner);
                if inner.closed {
                    return Selected::SendClosed(i);
                }
                match ch.try_send_locked(&mut inner, value) {
                    Ok(()) => return Selected::Sent(i),
                    Err(v) => {
                        drop(inner);
                        arms[i] = Some(Arm::Send(ch, v));
                    }
                }
            }
            Some(Arm::Timer(0)) => return Selected::Timer(i),
            other => arms[i] = other,
        }
    }
    if has_else {
        return Selected::Else;
    }
    if super::cancelled() {
        return cancelled_result(&is_recv);
    }
    // Wait on every arm with one shared token.
    let task = current().expect("select outside a task");
    let token = Arc::new(AtomicUsize::new(0));
    let mut waiters: Vec<Option<(Arc<Waiter<T>>, bool)>> = (0..n).map(|_| None).collect();
    for &i in &order {
        match arms[i].take() {
            Some(Arm::Recv(ch)) => {
                let w = Waiter::new(Arc::clone(&task), Arc::clone(&token), i, None);
                let mut inner = lock(&ch.inner);
                if (inner.closed || !inner.buffer.is_empty() || !inner.senders.is_empty())
                    && w.claim()
                {
                    let v = Chan::try_recv_locked(&mut inner);
                    drop(inner);
                    withdraw(&chans, &waiters);
                    return Selected::Recv(i, v);
                }
                inner.receivers.push_back(Arc::clone(&w));
                waiters[i] = Some((w, false));
            }
            Some(Arm::Send(ch, value)) => {
                let w = Waiter::new(Arc::clone(&task), Arc::clone(&token), i, Some(value));
                let mut inner = lock(&ch.inner);
                let ready =
                    inner.closed || inner.buffer.len() < ch.capacity || !inner.receivers.is_empty();
                if ready && w.claim() {
                    if inner.closed {
                        drop(inner);
                        withdraw(&chans, &waiters);
                        return Selected::SendClosed(i);
                    }
                    // The token is ours: hand the value over without claiming it again.
                    if let Some(value) = lock(&w.slot).take() {
                        let mut value = Some(value);
                        while let Some(r) = inner.receivers.pop_front() {
                            if r.claim() {
                                *lock(&r.slot) = value.take();
                                r.complete();
                                break;
                            }
                        }
                        if let Some(v) = value {
                            inner.buffer.push_back(v);
                        }
                    }
                    drop(inner);
                    withdraw(&chans, &waiters);
                    return Selected::Sent(i);
                }
                inner.senders.push_back(Arc::clone(&w));
                waiters[i] = Some((w, true));
            }
            Some(Arm::Timer(ns)) => {
                let at = sched.now().saturating_add(ns);
                let token = Arc::clone(&token);
                let target = Arc::clone(&task);
                sched.add_timer(
                    at,
                    Box::new(move || {
                        if token
                            .compare_exchange(0, i + 1, Ordering::AcqRel, Ordering::Acquire)
                            .is_ok()
                        {
                            wake(&target);
                        }
                    }),
                );
            }
            None => {}
        }
    }
    loop {
        let won = token.load(Ordering::Acquire);
        if won != 0 && won != CANCELLED {
            let arm = won - 1;
            // Wait until the winning operation has finished its transfer.
            if let Some((w, _)) = &waiters[arm] {
                while !w.done.load(Ordering::Acquire) {
                    super::park();
                }
            }
            withdraw(&chans, &waiters);
            return match &waiters[arm] {
                Some((w, true)) => {
                    if w.closed.load(Ordering::Acquire) {
                        Selected::SendClosed(arm)
                    } else {
                        Selected::Sent(arm)
                    }
                }
                Some((w, false)) => Selected::Recv(arm, lock(&w.slot).take()),
                None => Selected::Timer(arm),
            };
        }
        if park_cancellable()
            && token
                .compare_exchange(0, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            withdraw(&chans, &waiters);
            return cancelled_result(&is_recv);
        }
    }
}

/// The result of a `select` in a cancelled scope (spec §11.3 rule 5).
fn cancelled_result<T>(is_recv: &[bool]) -> Selected<T> {
    match is_recv.iter().position(|&r| r) {
        Some(i) => Selected::Recv(i, None),
        None => Selected::Cancelled,
    }
}

/// The waiters of a `select`, per arm: the waiter and whether it sends.
type Waiters<T> = [Option<(Arc<Waiter<T>>, bool)>];

/// Removes the waiters of a `select` from their channels.
fn withdraw<T>(chans: &[Option<&Chan<T>>], waiters: &Waiters<T>) {
    for (ch, waiter) in chans.iter().zip(waiters) {
        if let (Some(ch), Some((w, sender))) = (ch, waiter) {
            let mut inner = lock(&ch.inner);
            let queue = if *sender {
                &mut inner.senders
            } else {
                &mut inner.receivers
            };
            queue.retain(|x| !Arc::ptr_eq(x, w));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Arm, Chan, Selected, select};
    use crate::sched::{Mode, enter_scope, exit_scope, run, sleep, spawn};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn in_scope(seed: u64, body: impl FnOnce() + 'static) {
        run(
            Mode::Deterministic { seed },
            64 * 1024,
            64 * 1024,
            move || {
                let (scope, previous) = enter_scope(None);
                body();
                exit_scope(&scope, previous);
            },
        );
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn rendezvous_channel_passes_values_in_order() {
        for seed in 0..20 {
            let got = Rc::new(RefCell::new(Vec::new()));
            let g = Rc::clone(&got);
            in_scope(seed, move || {
                let ch: Rc<Chan<i32>> = Rc::new(Chan::new(0));
                let tx = Rc::clone(&ch);
                spawn(Box::new(move || {
                    for i in 0..5 {
                        assert!(tx.send(i).is_ok());
                    }
                    tx.close();
                }));
                while let Some(v) = ch.recv() {
                    g.borrow_mut().push(v);
                }
            });
            assert_eq!(*got.borrow(), vec![0, 1, 2, 3, 4], "seed {seed}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn workers_share_a_buffered_channel() {
        for seed in 0..20 {
            let total = Rc::new(RefCell::new(0));
            let t = Rc::clone(&total);
            in_scope(seed, move || {
                let jobs: Rc<Chan<i32>> = Rc::new(Chan::new(4));
                let results: Rc<Chan<i32>> = Rc::new(Chan::new(100));
                let (scope, previous) = enter_scope(None);
                for _ in 0..3 {
                    let (jobs, results) = (Rc::clone(&jobs), Rc::clone(&results));
                    spawn(Box::new(move || {
                        while let Some(j) = jobs.recv() {
                            let _ = results.send(j * j);
                        }
                    }));
                }
                for j in 1..=10 {
                    let _ = jobs.send(j);
                }
                jobs.close();
                exit_scope(&scope, previous);
                results.close();
                while let Some(v) = results.recv() {
                    *t.borrow_mut() += v;
                }
            });
            assert_eq!(*total.borrow(), 385, "seed {seed}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn select_with_timeout() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let l = Rc::clone(&log);
        in_scope(5, move || {
            let inbox: Rc<Chan<String>> = Rc::new(Chan::new(0));
            let tx = Rc::clone(&inbox);
            spawn(Box::new(move || {
                for i in 1..=3 {
                    sleep(10_000_000);
                    let _ = tx.send(format!("message {i}"));
                }
            }));
            loop {
                match select(vec![Arm::Recv(&inbox), Arm::Timer(200_000_000)], false) {
                    Selected::Recv(_, Some(m)) => l.borrow_mut().push(m),
                    Selected::Timer(_) => {
                        l.borrow_mut().push("idle".to_owned());
                        break;
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
        });
        assert_eq!(
            *log.borrow(),
            vec!["message 1", "message 2", "message 3", "idle"]
        );
    }

    #[test]
    #[cfg_attr(miri, ignore = "switches stacks with inline assembly")]
    fn cancelled_receiver_gets_none() {
        let got = Rc::new(RefCell::new(None));
        let g = Rc::clone(&got);
        run(
            Mode::Deterministic { seed: 2 },
            64 * 1024,
            64 * 1024,
            move || {
                let ch: Rc<Chan<i32>> = Rc::new(Chan::new(0));
                let (scope, previous) = enter_scope(Some(5_000_000));
                let rx = Rc::clone(&ch);
                spawn(Box::new(move || {
                    *g.borrow_mut() = Some(rx.recv());
                }));
                exit_scope(&scope, previous);
            },
        );
        assert_eq!(*got.borrow(), Some(None));
    }
}
