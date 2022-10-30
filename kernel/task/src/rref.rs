use crate::{get_my_current_task, ExitValue, KillReason, RunState, Task};
use alloc::{boxed::Box, sync::Arc};
use core::{
    any::Any,
    hash::{Hash, Hasher},
    ops::Deref,
    sync::atomic::Ordering,
};
use irq_safety::interrupts_enabled;

// FIXME Document.
#[derive(Debug)]
pub struct TaskRef<const JOINABLE: bool = false, const UNBLOCKABLE: bool = false> {
    pub(crate) task: Arc<Task>,
}

assert_not_impl_any!(TaskRef<true, false>: Clone);
assert_not_impl_any!(TaskRef<true, true>: Clone);

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> PartialEq for TaskRef<JOINABLE, UNBLOCKABLE> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.task, &other.task)
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> TaskRef<JOINABLE, UNBLOCKABLE> {
    #[allow(clippy::should_implement_trait)]
    pub fn eq<const J: bool, const U: bool>(&self, other: &TaskRef<J, U>) -> bool {
        Arc::ptr_eq(&self.task, &other.task)
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> Eq for TaskRef<JOINABLE, UNBLOCKABLE> {}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> Hash for TaskRef<JOINABLE, UNBLOCKABLE> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.task).hash(state);
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> Deref for TaskRef<JOINABLE, UNBLOCKABLE> {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        self.task.deref()
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> TaskRef<JOINABLE, UNBLOCKABLE> {
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> TaskRef<false, false> {
        TaskRef {
            task: self.task.clone(),
        }
    }
}

impl Clone for TaskRef<false, false> {
    fn clone(&self) -> Self {
        Self {
            task: self.task.clone(),
        }
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> TaskRef<JOINABLE, UNBLOCKABLE> {
    // FIXME: Move join to only be implemented for <true, UNBLOCKABLE>
    /// Blocks until this task has exited or has been killed.
    ///
    /// Returns `Ok()` once this task has exited,
    /// and `Err()` if there is a problem or interruption while waiting for it
    /// to exit.
    ///
    /// # Note
    /// * You cannot call `join()` on the current thread, because a thread
    ///   cannot wait for itself to finish running. This will result in an
    ///   `Err()` being immediately returned.
    /// * You cannot call `join()` with interrupts disabled, because it will
    ///   result in permanent deadlock (well, this is only true if the requested
    ///   `task` is running on the same cpu...  but good enough for now).
    pub fn join(&self) -> Result<(), &'static str> {
        let curr_task =
            get_my_current_task().ok_or("join(): failed to check what current task is")?;
        if Arc::ptr_eq(&self.task, &curr_task.task) {
            return Err("BUG: cannot call join() on yourself (the current task).");
        }

        if !interrupts_enabled() {
            return Err(
                "BUG: cannot call join() with interrupts disabled; it will cause deadlock.",
            );
        }

        // First, wait for this Task to be marked as Exited (no longer runnable).
        while !self.task.has_exited() {}

        // Then, wait for it to actually stop running on any CPU core.
        while self.task.is_running() {}

        Ok(())
    }

    /// Call this function to indicate that this task has successfully ran to
    /// completion, and that it has returned the given `exit_value`.
    ///
    /// This should only be used within task cleanup functions to indicate
    /// that the current task has cleanly exited.
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner
    /// state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.
    /// * Returns `Err` if this `Task` was already exited, and does not
    ///   overwrite the existing exit status.
    ///  
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run
    /// again.
    #[doc(hidden)]
    pub fn mark_as_exited(&self, exit_value: Box<dyn Any + Send>) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Completed(exit_value))
    }

    /// Call this function to indicate that this task has been cleaned up (e.g.,
    /// by unwinding) and it is ready to be marked as killed, i.e., it will
    /// never run again. This task (`self`) must be the currently executing
    /// task, you cannot invoke `mark_as_killed()` on a different task.
    ///
    /// If you want to kill another task, use the [`kill()`](method.kill) method
    /// instead.
    ///
    /// This should only be used within task cleanup functions (e.g., after
    /// unwinding) to indicate that the current task has crashed or failed
    /// and has been killed by the system.
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner
    /// state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.
    /// * Returns `Err` if this `Task` was already exited, and does not
    ///   overwrite the existing exit status.
    ///  
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run
    /// again.
    #[doc(hidden)]
    pub fn mark_as_killed(&self, reason: KillReason) -> Result<(), &'static str> {
        let curr_task =
            get_my_current_task().ok_or("mark_as_exited(): failed to check the current task")?;
        if Arc::ptr_eq(&curr_task.task, &self.task) {
            self.internal_exit(ExitValue::Killed(reason))
        } else {
            Err("`mark_as_exited()` can only be invoked on the current task, not on another task.")
        }
    }

    /// Kills this `Task` (not a clean exit) without allowing it to run to
    /// completion. The provided `KillReason` indicates why it was killed.
    ///
    /// **
    /// Currently this immediately kills the task without performing any
    /// unwinding cleanup. In the near future, the task will be unwound such
    /// that its resources are freed/dropped to ensure proper cleanup before
    /// the task is actually fully killed. **
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner
    /// state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set to the given
    ///   `KillReason`.
    /// * Returns `Err` if this `Task` was already exited, and does not
    ///   overwrite the existing exit status.
    ///
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run
    /// again.
    pub fn kill(&self, reason: KillReason) -> Result<(), &'static str> {
        // TODO FIXME: cause a panic in this Task such that it will start the unwinding
        // process instead of immediately causing it to exit
        self.internal_exit(ExitValue::Killed(reason))
    }

    /// The internal routine that actually exits or kills a Task.
    ///
    /// # Locking / Deadlock
    /// Obtains the lock on this `Task`'s inner state in order to mutate it.
    fn internal_exit(&self, val: ExitValue) -> Result<(), &'static str> {
        if self.task.has_exited() {
            return Err(
                "BUG: task was already exited! (did not overwrite its existing exit value)",
            );
        }
        {
            let mut inner = self.task.inner.lock();
            inner.exit_value = Some(val);
            self.task.runstate.store(RunState::Exited);

            // Corner case: if the task isn't currently running (as with killed tasks),
            // we must clean it up now rather than in `task_switch()`, as it will never be
            // scheduled in again.
            if !self.task.is_running() {
                trace!(
                    "internal_exit(): dropping TaskLocalData for non-running task {}",
                    &*self.task
                );
                drop(inner.task_local_data.take());
            }
        }

        #[cfg(runqueue_spillful)]
        {
            if let Some(remove_from_runqueue) = RUNQUEUE_REMOVAL_FUNCTION.get() {
                if let Some(rq) = self.on_runqueue() {
                    remove_from_runqueue(self, rq)?;
                }
            }
        }

        Ok(())
    }

    /// Converts a task reference into the desired kind. This function should
    /// only be used to _trick_ the type system.
    ///
    /// # Safety
    ///
    /// The constants must not change. See `spawn::TaskBuild::spawn` for example
    /// usage.
    pub unsafe fn into_kind<const J: bool, const U: bool>(self) -> TaskRef<J, U> {
        unsafe { self._into_kind() }
    }

    /// Converts a task reference into the desired kind.
    ///
    /// # Safety
    ///
    /// The transition must make logical sense.
    unsafe fn _into_kind<const J: bool, const U: bool>(self) -> TaskRef<J, U> {
        let s = core::mem::ManuallyDrop::new(self);
        // SAFETY: - s.task is a valid reference
        //         - the arc's strong count correctly remains unchanged
        let task = unsafe { core::ptr::read(&s.task) };
        TaskRef { task }
        // The caller has made changes to the task state for the transition to
        // make logical sense. We intentionally don't drop s.
    }
}

impl<const JOINABLE: bool> TaskRef<JOINABLE, true> {
    pub fn unblock(self) -> Result<TaskRef<JOINABLE, false>, TaskRef<JOINABLE, false>> {
        match self
            .task
            .runstate
            .compare_exchange(RunState::Blocked, RunState::Runnable)
        {
            // SAFETY: We just unblocked the task.
            Ok(_) => Ok(unsafe { self.into_kind() }),
            // SAFETY: The task was already not blocked.
            // TODO: Ideally we would transition the TaskRef into some kind of Killed state.
            Err(current) => {
                // We should only hit this if the task has been killed. No other task should be
                // able to unblock us.
                debug_assert_ne!(current, RunState::Runnable);
                Err(unsafe { self.into_kind() })
            }
        }
    }
}

impl<const JOINABLE: bool> TaskRef<JOINABLE, false> {
    fn _block(&self) -> Result<RunState, RunState> {
        self.task
            .runstate
            .compare_exchange(RunState::Runnable, RunState::Blocked)
    }

    pub fn block(self) -> Result<TaskRef<JOINABLE, true>, TaskRef<JOINABLE, false>> {
        match self._block() {
            Ok(_) => Ok(unsafe { self.into_kind() }),
            Err(_) => Err(self),
        }
    }

    /// Runs a closure and then blocks the task.
    ///
    /// This function must only be used to add a blocked task reference to a
    /// queue prior to blocking the task. However, to avoid race conditions,
    /// the lock on the queue must be obtained prior to calling
    /// `run_and_block`. `f` must operate on the guard.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # fn main() -> Result<(), &'static str> {
    /// # let queue = irq_safety::MutexIrqSafe::new(Vec::new());
    /// let task = task::get_my_current_task().ok_or("couldn't get current task")?;
    /// let mut guard = queue.lock();
    /// // SAFETY: blocked_task is not dropped.
    /// unsafe {
    ///     task.clone()
    ///         .run_and_block(|blocked_task| guard.push(blocked_task))
    /// }
    /// .map_err(|_| "couldn't block current task")?;
    /// drop(guard);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Safety
    ///
    /// `f` must not drop the provided `TaskRef`.
    pub unsafe fn run_and_block<F>(self, mut f: F) -> Result<(), ()>
    where
        F: FnMut(TaskRef<JOINABLE, true>),
    {
        let task = self.task.clone();
        f(unsafe { self.into_kind() });

        let task_ref: TaskRef<false, false> = TaskRef { task };
        match task_ref._block() {
            Ok(_) => Ok(()),
            Err(_) => Err(()),
        }
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> Drop for TaskRef<JOINABLE, UNBLOCKABLE> {
    fn drop(&mut self) {
        // Marks the inner [`Task`] as not joinable, meaning that it is an
        // orphaned task that will be auto-reaped after exiting.
        if JOINABLE {
            self.task.joinable.store(false, Ordering::Relaxed);
        }

        if UNBLOCKABLE {
            #[allow(clippy::collapsible_if)]
            if self
                .task
                .runstate
                .compare_exchange(RunState::Blocked, RunState::Runnable)
                .is_err()
            {
                warn!("TaskRef::drop(): failed to block task");
            }
        }
    }
}
