#![no_std]

use core::{
    pin::Pin,
    task::{Context, Poll},
};

use async_wait_queue::WaitQueue;
use futures::stream::{FusedStream, Stream};
use mpmc::Queue;
use sync::DeadlockPrevention;
use sync_spin::Spin;

#[derive(Clone)]
pub struct Channel<T, P = Spin>
where
    T: Send,
    P: DeadlockPrevention,
{
    inner: Queue<T>,
    senders: WaitQueue<P>,
    receivers: WaitQueue<P>,
}

impl<T, P> Channel<T, P>
where
    T: Send,
    P: DeadlockPrevention,
{
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Queue::with_capacity(capacity),
            senders: WaitQueue::new(),
            receivers: WaitQueue::new(),
        }
    }

    pub async fn send(&self, value: T) {
        let mut temp = Some(value);

        self.senders
            .wait_until(|| match self.inner.push(temp.take().unwrap()) {
                Ok(()) => {
                    self.receivers.notify_one();
                    Some(())
                }
                Err(value) => {
                    temp = Some(value);
                    None
                }
            })
            .await
    }

    pub fn try_send(&self, value: T) -> Result<(), T> {
        self.inner.push(value)
    }

    pub fn blocking_send(&self, value: T) {
        dreadnought::block_on(self.send(value))
    }

    pub async fn recv(&self) -> T {
        let value = self.receivers.wait_until(|| self.inner.pop()).await;
        self.senders.notify_one();
        value
    }

    pub fn blocking_recv(&self) -> T {
        dreadnought::block_on(self.recv())
    }
}

impl<T, P> Stream for Channel<T, P>
where
    T: Send,
    P: DeadlockPrevention,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self
            .receivers
            .poll_wait_until(ctx, &mut || self.inner.pop())
        {
            Poll::Ready(value) => {
                self.senders.notify_one();
                Poll::Ready(Some(value))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<T, P> FusedStream for Channel<T, P>
where
    T: Send,
    P: DeadlockPrevention,
{
    fn is_terminated(&self) -> bool {
        false
    }
}
