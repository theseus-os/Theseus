#![allow(clippy::new_without_default)]
#![no_std]

extern crate alloc;

use core::{
    future::poll_fn,
    task::{Context, Poll, Waker},
};

use alloc::sync::Arc;
use mpmc_queue::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;

#[derive(Clone)]
pub struct WaitQueue<P = Spin>
where
    P: DeadlockPrevention,
{
    inner: Arc<Queue<Waker, P>>,
}

impl<P> WaitQueue<P>
where
    P: DeadlockPrevention,
{
    /// Creates a new empty wait queue.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Queue::new()),
        }
    }

    pub async fn wait_until<F, T>(&self, mut condition: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        poll_fn(move |context| self.poll_wait_until(context, &mut condition)).await
    }

    pub fn poll_wait_until<F, T>(&self, ctx: &mut Context, condition: &mut F) -> Poll<T>
    where
        F: FnMut() -> Option<T>,
    {
        let wrapped_condition = || {
            if let Some(value) = condition() {
                Ok(value)
            } else {
                Err(())
            }
        };

        match self
            .inner
            // TODO: Lazy clone
            .push_if_fail(ctx.waker().clone(), wrapped_condition)
        {
            Ok(value) => Poll::Ready(value),
            Err(()) => Poll::Pending,
        }
    }

    pub fn blocking_wait_until<F, T>(&self, condition: F) -> T
    where
        F: FnMut() -> Option<T>,
    {
        dreadnought::block_on(self.wait_until(condition))
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
