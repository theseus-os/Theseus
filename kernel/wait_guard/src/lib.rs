#![no_std]

use task::{RunState, TaskRef};

/// An object that holds a blocked `Task` that will be automatically unblocked
/// upon drop.
pub struct WaitGuard {
    task: TaskRef,
}

impl WaitGuard {
    /// Blocks the given `Task` and returns a new `WaitGuard` object that will
    /// automatically unblock that Task when it is dropped.
    ///
    /// Returns an error if the task cannot be blocked; see
    /// [`task::Task::block()`] for more details.
    pub fn new(task: TaskRef) -> Result<Self, RunState> {
        task.block()?;
        Ok(WaitGuard { task })
    }

    /// Blocks the task guarded by this waitguard, which is useful to re-block a
    /// task after it spuriously woke up.
    ///
    /// Returns an error if the task cannot be blocked; see
    /// [`task::Task::block()`] for more details.
    pub fn block_again(&self) -> Result<RunState, RunState> {
        self.task.block()
    }

    /// Returns a reference to the `Task` being blocked in this `WaitGuard`.
    pub fn task(&self) -> &TaskRef {
        &self.task
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        self.task.unblock().unwrap();
    }
}
