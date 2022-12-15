//! Asynchronous operating system tasks.

use alloc::boxed::Box;
use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use task::{ExitValue, JoinableTaskRef, KillReason, PanicInfoOwned};

/// Spawn a new asynchronous task, returning a [`JoinHandle`] for it.
///
/// You do not need to poll the handle to make the task execute --- it will
/// start running in the background immediately.
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    let future = Box::pin(future);
    let task = spawn::new_task_builder(crate::block_on, future)
        .spawn()
        .unwrap();
    JoinHandle {
        task,
        phantom_data: PhantomData,
    }
}

/// An owned permission to join on a task.
pub struct JoinHandle<T> {
    pub(crate) task: JoinableTaskRef,
    pub(crate) phantom_data: PhantomData<T>,
}

impl<T> JoinHandle<T> {
    /// Abort the task associated with the handle.
    ///
    /// If the cancelled task was already completed at the time it was
    /// cancelled, it will return the successful result. Otherwise, polling the
    /// handle will fail with an [`Error::Cancelled`].
    pub fn abort(&self) {
        let _ = self.task.kill(KillReason::Requested);
    }

    /// Returns whether the task associated with the handle has finished.
    pub fn is_finished(&self) -> bool {
        self.task.has_exited()
    }
}

impl<T> Future for JoinHandle<T>
where
    T: 'static,
{
    type Output = Result<T>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.task.set_waker(context.waker().clone());
        if self.is_finished() {
            Poll::Ready(match self.task.retrieve_exit_value() {
                Some(exit_value) => match exit_value {
                    ExitValue::Completed(value) => {
                        // SAFETY: The task ran block_on which returns a T.
                        // TODO: Why doesn't T have to be Any?
                        Ok(Box::into_inner(unsafe { value.downcast_unchecked::<T>() }))
                    }
                    ExitValue::Killed(reason) => match reason {
                        KillReason::Requested => Err(Error::Cancelled),
                        KillReason::Panic(info) => Err(Error::Panic(info)),
                        KillReason::Exception(num) => Err(Error::Exception(num)),
                    },
                },
                None => Err(Error::Reaped),
            })
        } else {
            Poll::Pending
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

/// An error returned from polling a [`JoinHandle`].
#[derive(Debug)]
pub enum Error {
    Cancelled,
    Panic(PanicInfoOwned),
    Reaped,
    Exception(u8),
}
