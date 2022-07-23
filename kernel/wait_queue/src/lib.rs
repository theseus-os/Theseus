//! This crate contains a wait queue implementation.

#![feature(result_option_inspect)]

#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use irq_safety::MutexIrqSafe;
use task::TaskRef;

// TODO: Should this be in a seperate crate. It's not used by WaitQueue in any
// way.
/// An object that holds a blocked `Task`
/// that will be automatically unblocked upon drop.  
pub struct WaitGuard {
    task: TaskRef,
}

impl WaitGuard {
    /// Blocks the given `Task` and returns a new `WaitGuard` object
    /// that will automatically unblock that Task when it is dropped.
    pub fn new(task: TaskRef) -> WaitGuard {
        task.block();
        WaitGuard { task }
    }

    /// Blocks the task guarded by this waitguard,
    /// which is useful to re-block a task after it spuriously woke up.
    pub fn block_again(&self) {
        self.task.block();
    }

    /// Returns a reference to the `Task` being blocked in this `WaitGuard`.
    pub fn task(&self) -> &TaskRef {
        &self.task
    }
}
impl Drop for WaitGuard {
    fn drop(&mut self) {
        self.task.unblock();
    }
}

/// Errors that may occur while waiting on a waitqueue/condition/event.
#[derive(Debug, PartialEq)]
pub enum WaitError {
    NoCurrentTask,
    Interrupted,
    Timeout,
    SpuriousWakeup,
}

/// A queue in which multiple `Task`s can wait for other `Task`s to notify them.
///
/// This can be shared across multiple `Task`s by wrapping it in an `Arc`.
pub struct WaitQueue(MutexIrqSafe<VecDeque<&'static TaskRef>>);

// *****************************************************************************
// *********************** IMPORTANT IMPLEMENTATION NOTE ***********************
// *****************************************************************************
// All modification of task runstates must be performed atomically with respect
// to adding or removing those tasks to/from the wait queue itself. Otherwise,
// there could be interleavings that result in tasks not being notified
// properly, or not actually being put to sleep when being placed on the
// wait queue, or the task being switched away from after setting itself to
// blocked (when waiting) but before it can release its lock on the wait queue.
// (Because once a task is blocked, it can never run again and thus has no
// chance to release its wait queue lock, causing deadlock). Thus, we disable
// preemption (well, currently we disable interrupts) AND hold the wait queue
// lock while changing task runstate, which ensures that once the task is
// blocked it will always release its wait queue lock.
// *****************************************************************************

impl WaitQueue {
    /// Create a new empty WaitQueue.
    pub fn new() -> WaitQueue {
        WaitQueue::with_capacity(4)
    }

    /// Create a new empty WaitQueue.
    pub fn with_capacity(initial_capacity: usize) -> WaitQueue {
        WaitQueue(MutexIrqSafe::new(VecDeque::with_capacity(initial_capacity)))
    }

    /// Puts the current `Task` to sleep where it blocks on this `WaitQueue`
    /// until it is notified by another `Task`.
    ///
    /// This function blocks until the `Task` is woken up through the notify
    /// mechanism.
    pub fn wait(&self) -> Result<(), WaitError> {
        let current_task = task::get_my_current_task().ok_or(WaitError::NoCurrentTask)?;
        let mut wait_queue = self.0.lock();

        // TODO: I don't think spurious wakeups are possible.
        debug_assert!(
            !wait_queue.contains(&current_task),
            "task was already on wait queue (spurious wakeup?)"
        );
        wait_queue.push_back(current_task);
        current_task.block();

        scheduler::schedule();
        todo!();
    }

    /// Similar to [`wait`](#method.wait), but this function blocks until the
    /// given `condition` closure returns `Some(value)`, and then returns
    /// that `value` inside `Ok()`.
    ///
    /// The `condition` will be executed atomically with respect to the wait
    /// queue, which avoids the problem of a waiting task missing a "notify"
    /// from another task due to interleaving of instructions that may occur
    /// if the `condition` is checked when the wait queue lock is not held.
    pub fn wait_until<F, R>(&self, mut condition: F) -> Result<R, WaitError>
    where
        // NOTE: FnMut is implemented for all closures that implement Fn.
        F: FnMut() -> Option<R>,
    {
        let current_task = task::get_my_current_task().ok_or(WaitError::NoCurrentTask)?;

        loop {
            let mut wait_queue = self.0.lock();

            // TODO: This function will be called once prior to the current task being added
            // to the wait queue. Do we want this?
            if let Some(ret) = condition() {
                return Ok(ret);
            }

            // TODO: I don't think spurious wakeups are possible.
            debug_assert!(
                !wait_queue.contains(&current_task),
                "task was already on wait queue (spurious wakeup?)"
            );
            wait_queue.push_back(current_task);
            current_task.block();

            drop(wait_queue);
            scheduler::schedule();
        }
    }

    /// Wakes up the longest waiting task.
    ///
    /// Returns `true` if a task was succesfuly woken up, false otherwise.
    pub fn notify_one(&self) -> bool {
        let mut wait_queue = self.0.lock();
        wait_queue
            .pop_front()
            .inspect(|task| task.unblock())
            .is_some()
    }

    /// Wake up a specific `Task` that is waiting on this queue.
    ///
    /// Returns `true` if the given task was in the queue, false otherwise.
    pub fn notify_specific(&self, task_to_wakeup: &TaskRef) -> bool {
        let mut wait_queue = self.0.lock();
        let index = wait_queue.iter().position(|task| *task == task_to_wakeup);
        index
            .and_then(|index| wait_queue.remove(index))
            .inspect(|task| task.unblock())
            .is_some()
    }
}
