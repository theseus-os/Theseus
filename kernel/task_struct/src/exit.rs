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
            Err("")
        } else {
            *self.exit_value_mailbox.lock() = Some(exit_value);
            self.runstate.store(RunState::Exited);

            atomic::fence(Ordering::Release);

            if let Some(waker) = self.inner.inner.lock().waker.take() {
                waker.wake();
            }

            if !self.is_running() {
                todo!();
            }

            Ok(())
        }
    }

    pub fn reap_if_orphaned(&self) {
        todo!();
        // if !self.is_joinable() {
        //     let _ = self.reap_exit_value();
        // }
    }

    #[doc(hidden)]
    pub fn obtain_for_unwinder(task: RawTaskRef) -> (Self, FailureCleanupFunction) {
        // FIXME: Check that current task.
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
