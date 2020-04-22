//! An asynchronous channel for Inter-Task Communication (ITC) with an internal queue for buffering messages.
//! 
//! This crate offers an asynchronous channel that allows multiple tasks
//! to exchange messages through the use of a bounded-capacity intermediate buffer.
//! Unlike the `rendezvous` channel, the sender and receiver do not need to rendezvous to send or receive data.
//! 
//! Only `Send` types can be sent or received through the channel.
//! 
//! This is not a zero-copy channel; 
//! to avoid copying large messages, use a reference (layer of indirection) like `Box`.

#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
// #[macro_use] extern crate debugit;
extern crate wait_queue;
extern crate mpmc;
extern crate atomic;

use core::sync::atomic::Ordering;
use alloc::sync::Arc;
use mpmc::Queue as MpmcQueue;
use wait_queue::{WaitQueue,WaitError};
use atomic::Atomic;


/// Create a new channel that allows senders and receivers to 
/// asynchronously exchange messages via an internal intermediary buffer.
/// 
/// This channel's buffer has a bounded capacity of minimum size 2 messages,
/// and it must be a power of 2 due to the restrictions of the current MPMC queue type that is used. 
/// The given `minimum_capacity` will be rounded up to the next largest power of 2, with a minimum value of 2.
/// 
/// When the number of pending (buffered) messages is larger than the capacity,
/// the channel is considered full.
/// Depending on whether a non-blocking or blocking send function is invoked,
/// future attempts to send another message will either block or return a `Full` error 
/// until the channel's buffer is drained by a receiver and space in the buffer becomes available.
/// 
/// `channel_status` indicates whether one end has been dropped. So that other end can respond.
/// 
/// Returns a tuple of `(Sender, Receiver)`.
pub fn new_channel<T: Send>(minimum_capacity: usize) -> (Sender<T>, Receiver<T>) {
    let channel = Arc::new(Channel::<T> {
        queue: MpmcQueue::with_capacity(minimum_capacity),
        waiting_senders: WaitQueue::new(),
        waiting_receivers: WaitQueue::new(),
        channel_status: Atomic::new(ChannelStatus::Connected)
    });
    (
        Sender   { channel: channel.clone() },
        Receiver { channel: channel }
    )
}

/// Possible values for a channel Endpoint.
/// Active : Initially channel is created with Active status.
/// Dropped : Set to dropped when one end is dropped.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ChannelStatus {
    Connected,
    Disconnected,
}

/// The inner channel for asynchronous communication between `Sender`s and `Receiver`s.
///
/// This struct is effectively a wrapper around a MPMC queue 
/// with waitqueues for senders (producers) and receivers (consumers).
/// 
/// This channel object is not Send/Sync or cloneable itself;
/// it can be shared across tasks using an `Arc`.
struct Channel<T: Send> {
    queue: MpmcQueue<T>,
    waiting_senders: WaitQueue,
    waiting_receivers: WaitQueue,
    channel_status : Atomic<ChannelStatus>
}


/// The sender (transmit) side of a channel.
pub struct Sender<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Sender<T> {
    /// Send a message, blocking until space in the channel's buffer is available. 
    /// 
    /// Returns `Ok(())` if the message was sent and received successfully,
    /// otherwise returns an error. 
    pub fn send(&self, msg: T) -> Result<(), &'static str> {
        // trace!("async_channel: send() entry");
        // Fast path: attempt to send the message, assuming the buffer isn't full
        let msg = match self.try_send(msg) {
            Ok(()) => return Ok(()),
            Err(returned_msg) => returned_msg,
        };

        // Slow path: the buffer was full, so now we need to block until space becomes available.
        // trace!("waiting for space to send...");

        // Here we use an option to store the un-sent message outside of the `closure`
        // so that we can repeatedly try to re-send it upon the next invocation of the `closure`
        // (which happens when this sender task is notified in the future).
        let mut msg = Some(msg);

        // This closure is invoked from within a locked context, so we cannot just call `try_send()` here
        // because it will notify the receivers which can cause deadlock.
        // Therefore, we need to perform the nofity action outside of this closure after it returns.
        let mut closure = || {
            let owned_msg = msg.take();
            let result = owned_msg.and_then(|m| match self.channel.queue.push(m) {
                Ok(()) => {
                    // trace!("Sending in closure");
                    Some(())
                },
                Err(returned_msg) => {
                    // Here: we (the sender) woke up and failed to send, 
                    // so we save the returned message outside of the closure to retry later. 
                    // trace!("try_send() failed, saving message {:?} for next retry.", debugit!(returned_msg));
                    msg = Some(returned_msg);
                    None
                }
            });

            if self.channel.channel_status.load(Ordering::SeqCst) == ChannelStatus::Disconnected {
                 // trace!("Receiver Endpoint is dropped");
                 // Here the receiver end has dropped. 
                 // So we don't wait anymore in the waitqueue
                 Err(())
            } else {
                Ok(result)
            }
            
        };

        let res = self.channel.waiting_senders
            .wait_until_mut(&mut closure)
            .map_err(|error| {
                if error == WaitError::EndpointDropped {
                    "Receiver Endpoint is dropped"
                } else {
                    "failed to add current task to queue of waiting senders waitqueue"
                }
            });
        // trace!("... sending space became available.");

        // If we successfully sent a message, we need to notify any waiting receivers.
        // As stated above, to avoid deadlock, this must be done here rather than in the above closure.
        if res.is_ok() {
            // trace!("successful send() is notifying receivers.");
            self.channel.waiting_receivers.notify_one();
        }
        res
    }

    /// Tries to send the message, only succeeding if buffer space is available.
    /// 
    /// If no buffer space is available, it returns the `msg` back to the caller without blocking. 
    pub fn try_send(&self, msg: T) -> Result<(), T> {
        match self.channel.queue.push(msg) {
            // successfully sent
            Ok(()) => {
                // trace!("successful try_send() is notifying receivers.");
                self.channel.waiting_receivers.notify_one();
                Ok(())
            }
            // queue was full, return message back to caller
            returned_msg => returned_msg,
        }
    }
}

/// The receiver side of a channel.
pub struct Receiver<T: Send> {
    channel: Arc<Channel<T>>,
}
impl <T: Send> Receiver<T> {
    /// Receive a message, blocking until a message is available in the buffer.
    /// 
    /// Returns the message if it was received properly, otherwise returns an error.
    pub fn receive(&self) -> Result<T, &'static str>  {
        // trace!("async_channel: receive() entry");
        // Fast path: attempt to receive a message, assuming the buffer isn't empty
        if let Some(msg) = self.try_receive_fast() {
            return Ok(msg);
        }

        // Slow path: the buffer was empty, so we need to block until a message is sent.
        // trace!("waiting to receive a message...");
        
        // This closure is invoked from within a locked context, so we cannot just call `try_receive()` here
        // because it will notify the receivers which can cause deadlock.
        // Therefore, we need to perform the nofity action outside of this closure after it returns.
        let res = self.channel.waiting_receivers
            .wait_until(&|| self.try_receive())
            .map_err(|error| {
                if error == WaitError::EndpointDropped {
                    "Sender Endpoint is dropped"
                } else {
                    "failed to add current task to queue of waiting receivers waitqueue"
                }
            });
        // trace!("... received msg.");

        // If we successfully received a message, we need to notify any waiting senders.
        // As stated above, to avoid deadlock, this must be done here rather than in the above closure.
        if res.is_ok() {
            // trace!("async_channel: successful receive() is notifying senders.");
            self.channel.waiting_senders.notify_one();
        }
        res
    }

    /// Tries to receive a message, only succeeding if a message is already available in the buffer.
    /// 
    /// If no such message exists, it returns `None` without blocking.
    pub fn try_receive_fast(&self) -> Option<T> {
        let msg = self.channel.queue.pop();
        if msg.is_some() {
            // trace!("successful try_receive_fast() is notifying senders.");
            self.channel.waiting_senders.notify_one();
            msg
        } else {
            None
        }
    }

    /// Tries to receive a message, only succeeding if a message is already available in the buffer.
    /// 
    /// If an endpoint is disconnected returns Err().
    /// If no such message exists, it returns `Ok(None)` without blocking.
    pub fn try_receive(&self) -> Result<Option<T>,()> {
        let msg = self.channel.queue.pop();
        if msg.is_some() {
            // trace!("successful try_receive() is notifying senders.");
            self.channel.waiting_senders.notify_one();
            Ok(msg)
        } else {
            if self.channel.channel_status.load(Ordering::SeqCst) == ChannelStatus::Disconnected {
                return Err(())
            }
            Ok(None)
        }
    }
}


/// Drop implementation marks the channel state and notifys the `Sender`
impl<T: Send> Drop for Receiver<T> {
    fn drop(&mut self) {
        // trace!("Dropping the receiver");
        self.channel.channel_status.store(ChannelStatus::Disconnected, Ordering::Release);
        self.channel.waiting_senders.notify_one();
    }
}

/// Drop implementation marks the channel state and notifys the `Receiver`
impl<T: Send> Drop for Sender<T> {
    fn drop(&mut self) {
        // trace!("Dropping the sender");
        self.channel.channel_status.store(ChannelStatus::Disconnected, Ordering::Release);
        self.channel.waiting_receivers.notify_one();
    }
}