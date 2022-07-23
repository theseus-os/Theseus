//! This crate contains a semaphore implementation.
//!
//! The implementation is based on the following sources:
//! - [https://people.mpi-sws.org/~druschel/courses/os/lectures/proc4.pdf]
//! - [https://www.cs.cornell.edu/courses/cs4410/2018su/lectures/lec07-sema.html]
//! - [https://www.cs.brandeis.edu/~cs146a/rust/doc-02-21-2015/nightly/std/sync/struct.Semaphore.html]
//! - [https://github.com/hermitcore/libhermit-rs/blob/master/src/synch/semaphore.rs]

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use task::{get_my_current_task, TaskRef};

// FIXME: THIS IS ALL WRONG. IT ASSUMES SINGLE THREAD :).

/// A counting, blocking, semaphore.
///
/// Semaphores are a form of atomic counter where access is only granted if the
/// counter is a positive value. Each acquisition will block the calling thread
/// until the counter is positive, and each release will increment the counter
/// and unblock any threads if necessary.
#[derive(Default)]
pub struct Semaphore(MutexIrqSafe<State>);

/// The internal state of a semaphore.
#[derive(Default)]
struct State {
    count: isize,
    queue: VecDeque<&'static TaskRef>,
}

impl Semaphore {
    /// Creates a new semaphore with the given `count`.
    pub fn new(count: isize) -> Self {
        Self(MutexIrqSafe::new(State {
            count,
            queue: VecDeque::new(),
        }))
    }

    /// Acquire a resource from this semaphore, blocking the current thread.
    ///
    /// This function is commonly referred to as `p` in literature.
    pub fn acquire(&self) {
        // TODO: crossbeam_utils::Backoff
        // TODO: use kernel::wait_queue

        let mut state = self.0.lock();
        if state.count > 0 {
            state.count -= 1;
            return;
        }

        // FIXME: Unwrap
        let current_task = get_my_current_task().unwrap();
        state.queue.push_back(current_task);
        drop(state);
        current_task.block();
        
        scheduler::schedule();
    }

    /// Release a resource from this semaphore.
    ///
    /// This function is commonly referred to as `v` in literature.
    pub fn release(&self) {
        let mut state = self.0.lock();
        if state.queue.is_empty() {
            state.count += 1;
        } else {
            // FIXME: Unwrap
            let waiting_task = state.queue.pop_front().unwrap();
            waiting_task.unblock();
        }
    }
}
