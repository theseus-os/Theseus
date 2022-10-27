use super::*;

// /// Represents a joinable [`TaskRef`], created by [`TaskRef::new()`].
// /// Auto-derefs into a [`TaskRef`].
// ///
// /// This allows another task to:
// /// * [`join`] this task, i.e., wait for this task to finish executing,
// /// * to obtain its [exit value] after it has completed.
// ///
// /// ## [`Drop`]-based Behavior
// /// The contained [`Task`] is joinable until this object is dropped.
// /// When dropped, this task will be marked as non-joinable and treated as an "orphan" task.
// /// This means that there is no way for another task to wait for it to complete
// /// or obtain its exit value.
// /// As such, this task will be auto-reaped after it exits (in order to avoid zombie tasks).
// ///
// /// ## Not `Clone`-able
// /// Due to the above drop-based behavior, this type must not implement `Clone`
// /// because it assumes there is only ever one `JoinableTaskRef` per task.
// ///
// /// However, this type auto-derefs into an inner [`TaskRef`], which *can* be cloned.
// ///
// // /// Note: this type is considered an internal implementation detail.
// // /// Instead, use the `TaskJoiner` type from the `spawn` crate,
// // /// which is intended to be the public-facing interface for joining a task.

/// A shareable, cloneable reference to a `Task` that exposes more methods
/// for task management and auto-derefs into an immutable `&Task` reference.
///
/// The `TaskRef` type is necessary because in many places across Theseus,
/// a reference to a Task is used.
/// For example, task lists, task spawning, task management, scheduling, etc.
///
/// ## Equality comparisons
/// `TaskRef` implements the [`PartialEq`] and [`Eq`] traits to ensure that
/// two `TaskRef`s are considered equal if they point to the same underlying `Task`.
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

// There is one impostor among us.
impl<const JOINABLE: bool, const UNBLOCKABLE: bool> TaskRef<JOINABLE, UNBLOCKABLE> {
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

// / Creates a new `TaskRef`, a shareable wrapper around the given `Task`.
// /
// / This function also initializes the given `Task`'s `TaskLocalData` struct,
// / which will be used to determine the current `Task` on each CPU.
// /
// / It does *not* add this task to the system-wide task list or any runqueues,
// / nor does it schedule this task in.
// /
// / ## Return
// / Returns a [`JoinableTaskRef`], which derefs into the newly-created `TaskRef`
// / and can be used to "join" this task (wait for it to exit) and obtain its exit value.

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> TaskRef<JOINABLE, UNBLOCKABLE> {
    // FIXME: Why can non-joinable task refs join??

    /// Blocks until this task has exited or has been killed.
    ///
    /// Returns `Ok()` once this task has exited,
    /// and `Err()` if there is a problem or interruption while waiting for it to exit.
    ///
    /// # Note
    /// * You cannot call `join()` on the current thread, because a thread cannot wait for itself to finish running.
    ///   This will result in an `Err()` being immediately returned.
    /// * You cannot call `join()` with interrupts disabled, because it will result in permanent deadlock
    ///   (well, this is only true if the requested `task` is running on the same cpu...  but good enough for now).
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

    /// Call this function to indicate that this task has successfully ran to completion,
    /// and that it has returned the given `exit_value`.
    ///
    /// This should only be used within task cleanup functions to indicate
    /// that the current task has cleanly exited.
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status.
    ///  
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run again.
    #[doc(hidden)]
    pub fn mark_as_exited(&self, exit_value: Box<dyn Any + Send>) -> Result<(), &'static str> {
        self.internal_exit(ExitValue::Completed(exit_value))
    }

    /// Call this function to indicate that this task has been cleaned up (e.g., by unwinding)
    /// and it is ready to be marked as killed, i.e., it will never run again.
    /// This task (`self`) must be the currently executing task,
    /// you cannot invoke `mark_as_killed()` on a different task.
    ///
    /// If you want to kill another task, use the [`kill()`](method.kill) method instead.
    ///
    /// This should only be used within task cleanup functions (e.g., after unwinding) to indicate
    /// that the current task has crashed or failed and has been killed by the system.
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status.
    ///  
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run again.
    #[doc(hidden)]
    pub fn mark_as_killed(&self, reason: KillReason) -> Result<(), &'static str> {
        let curr_task =
            get_my_current_task().ok_or("mark_as_exited(): failed to check the current task")?;
        if curr_task == self.to_simple() {
            self.internal_exit(ExitValue::Killed(reason))
        } else {
            Err("`mark_as_exited()` can only be invoked on the current task, not on another task.")
        }
    }

    /// Kills this `Task` (not a clean exit) without allowing it to run to completion.
    /// The provided `KillReason` indicates why it was killed.
    ///
    /// **
    /// Currently this immediately kills the task without performing any unwinding cleanup.
    /// In the near future, the task will be unwound such that its resources are freed/dropped
    /// to ensure proper cleanup before the task is actually fully killed.
    /// **
    ///
    /// # Locking / Deadlock
    /// This method obtains a writable lock on the underlying Task's inner state.
    ///
    /// # Return
    /// * Returns `Ok` if the exit status was successfully set to the given `KillReason`.     
    /// * Returns `Err` if this `Task` was already exited, and does not overwrite the existing exit status.
    ///
    /// # Note
    /// The `Task` will not be halted immediately --
    /// it will finish running its current timeslice, and then never be run again.
    pub fn kill(&self, reason: KillReason) -> Result<(), &'static str> {
        // TODO FIXME: cause a panic in this Task such that it will start the unwinding process
        // instead of immediately causing it to exit
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
            // we must clean it up now rather than in `task_switch()`, as it will never be scheduled in again.
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

    fn to_simple(&self) -> &TaskRef<false, false> {
        // FIXME: Double check that this is ok.
        unsafe { core::mem::transmute(self) }
    }
}

impl<const JOINABLE: bool> TaskRef<JOINABLE, true> {
    pub fn unblock(self) -> (TaskRef<JOINABLE, false>, bool) {
        let result = self
            .task
            .runstate
            .compare_exchange(RunState::Blocked, RunState::Runnable);
        (unsafe { core::mem::transmute(self) }, result.is_ok())
    }
}

impl<const JOINABLE: bool> TaskRef<JOINABLE, false> {
    pub fn block(self) -> Result<TaskRef<JOINABLE, true>, TaskRef<JOINABLE, false>> {
        if self
            .task
            .runstate
            .compare_exchange(RunState::Runnable, RunState::Blocked)
            .is_ok()
        {
            Ok(unsafe { core::mem::transmute(self) })
        } else {
            Err(self)
        }
    }
}

impl<const JOINABLE: bool, const UNBLOCKABLE: bool> Drop for TaskRef<JOINABLE, UNBLOCKABLE> {
    /// Marks the inner [`Task`] as not joinable, meaning that it is an orphaned task
    /// that will be auto-reaped after exiting.
    fn drop(&mut self) {
        if JOINABLE {
            self.task.joinable.store(false, Ordering::Relaxed);
        }

        if UNBLOCKABLE {
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
