#![allow(clippy::new_without_default)]
#![no_std]

use mpmc_queue::Queue;
use preemption::hold_preemption;
use sync::DeadlockPrevention;
use sync_spin::Spin;
use task::{get_my_current_task, TaskRef};

/// A queue of tasks.
///
/// When tasks are popped off the front of the queue using [`notify_one`] or
/// [`notify_all`] they are unblocked.
///
/// The wait queue uses a mutex internally and hence exposes a deadlock
/// prevention type parameter. By default it is set to [`Spin`]. It should only
/// be set to another deadlock prevention method if a spin mutex could lead to a
/// deadlock, for example in an interrupt context. You do not need to use
/// `DisablePreemption` deadlock prevention to avoid scheduler race conditions -
/// they are already accounted for in the [`Spin`] implementation.
///
/// [`notify_one`]: Self::notify_one
/// [`notify_all`]: Self::notify_all
pub struct WaitQueue<P = Spin>
where
    P: DeadlockPrevention,
{
    inner: Queue<P, TaskRef>,
}

impl<P> WaitQueue<P>
where
    P: DeadlockPrevention,
{
    /// Creates a new wait queue.
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    /// Blocks the current task until the given condition succeeds.
    pub fn wait_until<F, T>(&self, mut condition: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        let task = get_my_current_task().unwrap();
        loop {
            let wrapped_condition = || {
                if let Some(value) = condition() {
                    Ok(value)
                } else {
                    // Prevents us from being preempted after blocking ourselves, but before we
                    // release the internal lock of the queue.
                    let preemption_guard = hold_preemption();
                    task.block().unwrap();
                    Err(preemption_guard)
                }
            };

            match self.inner.push_if_fail(task.clone(), wrapped_condition) {
                Ok(value) => return value,
                Err(preemption_guard) => {
                    drop(preemption_guard);
                    scheduler::schedule();
                }
            }
        }
    }

    /// Notifies the first task in the wait queue.
    ///
    /// If it fails to unblock the first task, it will continue unblocking
    /// subsequent tasks until a task is successfully unblocked.
    pub fn notify_one(&self) -> bool {
        loop {
            let task = match self.inner.pop() {
                Some(task) => task,
                None => return false,
            };

            if task.unblock().is_ok() {
                return true;
            }
        }
    }

    /// Notifies all the tasks in the wait queue.
    pub fn notify_all(&self) {
        while self.notify_one() {}
    }
}
