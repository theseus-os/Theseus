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

/// A bounded, multi-producer, multi-consumer asynchronous channel.
///
/// The channel can also be used outside of an asynchronous runtime with the
/// [`blocking_send`], and [`blocking_recv`] methods.
///
/// [`blocking_send`]: Self::blocking_send
/// [`blocking_recv`]: Self::blocking_recv
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
    /// Creates a new channel.
    ///
    /// The provided capacity dictates how many messages can be stored in the
    /// queue before the sender blocks.
    ///
    /// # Examples
    ///
    /// ```
    /// use async_channel::Channel;
    ///
    /// let channel = Channel::new(2);
    ///
    /// assert!(channel.try_send(1).is_ok());
    /// assert!(channel.try_send(2).is_ok());
    /// // The channel is full.
    /// assert!(channel.try_send(3).is_err());
    ///
    /// assert_eq!(channel.try_recv(), Some(1));
    /// assert_eq!(channel.try_recv(), Some(2));
    /// assert!(channel.try_recv().is_none());
    /// ```
    // TODO: Is a capacity of 0 = rendezvous?
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Queue::with_capacity(capacity),
            senders: WaitQueue::new(),
            receivers: WaitQueue::new(),
        }
    }

    /// Sends `value`.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe, in that if it is dropped prior to
    /// completion, `value` is guaranteed to have not been set. However, in that
    /// case `value` will be dropped.
    pub async fn send(&self, value: T) {
        let mut temp = Some(value);

        self.senders
            .wait_until(|| match self.inner.push(temp.take().unwrap()) {
                Ok(()) => {
                    log::info!("succesfully sent message");
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

    /// Tries to send `value`.
    ///
    /// # Errors
    ///
    /// Returns an error containing `value` if the channel was full.
    pub fn try_send(&self, value: T) -> Result<(), T> {
        self.inner.push(value)?;
        self.receivers.notify_one();
        Ok(())
    }

    /// Blocks the current thread until `value` is sent.
    pub fn blocking_send(&self, value: T) {
        dreadnought::block_on(self.send(value))
    }

    /// Receives the next value.
    ///
    /// # Cancel safety
    ///
    /// This method is cancel safe.
    pub async fn recv(&self) -> T {
        let value = self.receivers.wait_until(|| self.inner.pop()).await;
        self.senders.notify_one();
        value
    }

    /// Tries to receive the next value.
    pub fn try_recv(&self) -> Option<T> {
        let value = self.inner.pop()?;
        self.senders.notify_one();
        Some(value)
    }

    /// Blocks the current thread until a value is received.
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
        // NOTE: If we ever implement disconnections, this will need to be modified.
        false
    }
}
