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
use wait_queue::WaitQueue;
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

/// Indicates whether channel is Connected or Disconnected
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum ChannelStatus {
    /// Channel is working. Initially channel is created with Connected status.
    Connected,
    /// Set to Disconnected when one end is dropped.
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
    channel_status: Atomic<ChannelStatus>
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
            Err((returned_msg, channel_status)) => {
                if channel_status == ChannelStatus::Disconnected {
                    return Err("Receiver Endpoint is dropped");
                }
                returned_msg
            },
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
                    Some(Ok(()))
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
                 Some(Err(()))
            } else {
                result
            }
            
        };

        // When wait returns it can be either a successful send marked as  Ok(Ok()), 
        // Error in the condition (channel disconnection) marked as Ok(Er()),
        // or the wait_until runs into error (Err()) 
        let res =  match self.channel.waiting_senders
            .wait_until_mut(&mut closure) {
                Ok(result) => {
                    match result {
                        Ok(()) => Ok(()),
                        Err(()) => Err("Receiver Endpoint is dropped"),
                    }
                },
                Err(_) => {
                    Err("failed to add current task to queue of waiting senders. Waitqueue returned unexpectedly")
                }
            };

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
    /// If no buffer space is available, it returns the `msg`  with `ChannelStatus` back to the caller without blocking. 
    pub fn try_send(&self, msg: T) -> Result<(), (T, ChannelStatus)> {
        // first we'll check whether the channel is active
        if self.channel.channel_status.load(Ordering::SeqCst) == ChannelStatus::Disconnected {
                return Err((msg, ChannelStatus::Disconnected));
        }

        match self.channel.queue.push(msg) {
            // successfully sent
            Ok(()) => {
                // trace!("successful try_send() is notifying receivers.");
                self.channel.waiting_receivers.notify_one();
                Ok(())
            }
            // queue was full, return message back to caller
            Err(returned_msg) => Err((returned_msg, ChannelStatus::Connected)),
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
    pub fn receive(&self) -> Result<T, &'static str> {
        // trace!("async_channel: receive() entry");
        // Fast path: attempt to receive a message, assuming the buffer isn't empty
        match self.try_receive() {
            Some(Ok(msg)) => {
                return Ok(msg);
            },
            Some(Err(_)) => {
                return Err("Sender Endpoint is dropped");
            },
            _ => {},
        };

        // Slow path: the buffer was empty, so we need to block until a message is sent.
        // trace!("waiting to receive a message...");
        
        // This closure is invoked from within a locked context, so we cannot just call `try_receive()` here
        // because it will notify the receivers which can cause deadlock.
        // Therefore, we need to perform the nofity action outside of this closure after it returns.
        // When wait returns it can be either a successful receiver marked as  Ok(Ok()), 
        // Error in wait condition marked as Ok(Er()),
        // or the wait_until runs into error (Err()) 
        let res =  match self.channel.waiting_receivers
            .wait_until(&|| self.try_receive()) {
                Ok(result) => {
                    match result {
                        Ok(msg) => Ok(msg),
                        Err(_) => Err("Sender Endpoint is dropped"),
                    }
                },
                Err(_) => {
                    Err("failed to add current task to queue of waiting receivers. Waitqueue returned unexpectedly")
                }
            };
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
    /// If receive succeeds returns `Some(Ok(T))`. 
    /// If an endpoint is disconnected returns `Some(Err(ChannelStatus::Disconnected))`. 
    /// If no such message exists, it returns `None` without blocking
    pub fn try_receive(&self) -> Option<Result<T, ChannelStatus>> {
        let msg = self.channel.queue.pop();
        if msg.is_some() {
            // trace!("successful try_receive() is notifying senders.");
            self.channel.waiting_senders.notify_one();
            Some(Ok(msg.unwrap()))
        } else {
            if self.channel.channel_status.load(Ordering::SeqCst) == ChannelStatus::Disconnected {
                return Some(Err(ChannelStatus::Disconnected))
            }
            None
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