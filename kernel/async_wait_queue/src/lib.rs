#![allow(clippy::new_without_default)]
#![no_std]

use core::{
    future::poll_fn,
    task::{Context, Poll, Waker},
};

use mpmc_queue::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;

pub struct WaitQueue<P = Spin>
where
    P: DeadlockPrevention,
{
    inner: Queue<Waker, P>,
}

impl<P> WaitQueue<P>
where
    P: DeadlockPrevention,
{
    /// Creates a new empty wait queue.
    pub const fn new() -> Self {
        Self {
            inner: Queue::new(),
        }
    }

    /// Blocks the current task until the given condition succeeds.
    pub async fn wait_until<F, T>(&self, mut condition: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        poll_fn(|context| self.wait_until_raw(context, &mut condition)).await
    }

    // TODO: Don't take ref to condition
    fn wait_until_raw<F, T>(&self, context: &mut Context, condition: &mut F) -> Poll<T>
    where
        F: FnMut() -> Option<T>,
    {
        let wrapped_condition = || {
            if let Some(value) = (*condition)() {
                Ok(value)
            } else {
                Err(())
            }
        };

        match self
            .inner
            // TODO: Lazy clone
            .push_if_fail(context.waker().clone(), wrapped_condition)
        {
            Ok(value) => Poll::Ready(value),
            Err(()) => Poll::Pending,
        }
    }

    /// Notifies the first task in the wait queue.
    ///
    /// Returns whether or not a task was awoken.
    pub fn notify_one(&self) -> bool {
        match self.inner.pop() {
            Some(waker) => {
                waker.wake();
                true
            }
            None => false,
        }
    }

    /// Notifies all the tasks in the wait queue.
    pub fn notify_all(&self) {
        while self.notify_one() {}
    }
}
