//! Defines an exitable task reference.

use core::{
    ops::Deref,
    sync::atomic::{self, Ordering},
};

use crate::{ExitValue, FailureCleanupFunction, RawTaskRef, RunState};

#[derive(Debug)]
pub struct ExitableTaskRef {
    inner: RawTaskRef,
}

impl ExitableTaskRef {
    pub fn mark_as_exited(&self, exit_value: ExitValue) -> Result<(), &'static str> {
        if self.has_exited() {
            Err("BUG: task was already exited! (did not overwrite its existing exit value)")
        } else {
            *self.exit_value_mailbox.lock() = Some(exit_value);
            self.runstate.store(RunState::Exited);

            atomic::fence(Ordering::Release);

            if let Some(waker) = (***self).inner.lock().waker.take() {
                waker.wake();
            }

            if !self.is_running() {
                todo!(
                    "Unhandled scenario: internal_exit(): task {self:?} wasn't running \
                    but its current task TLS variable needs to be cleaned up!",
                );
                // Note: we cannot call `deinit_current_task()` here because if this task
                //       isn't running, then it's definitely not the current task.
                //
                // let _taskref_in_tls = deinit_current_task();
                // drop(_taskref_in_tls);
            }

            Ok(())
        }
    }

    pub fn reap_if_orphaned(&self) {
        if !self.is_joinable() {
            let _ = self.reap_exit_value();
        }
    }

    /// Creates an exitable task reference from a raw task reference.
    ///
    /// # Safety
    ///
    /// This function should only be called in `init_current_task` or the unwinder.
    #[doc(hidden)]
    pub unsafe fn from_raw(task: RawTaskRef) -> (Self, FailureCleanupFunction) {
        let f = task.failure_cleanup_function;
        (Self { inner: task }, f)
    }
}

impl Deref for ExitableTaskRef {
    type Target = RawTaskRef;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl !Send for ExitableTaskRef {}

impl !Sync for ExitableTaskRef {}
