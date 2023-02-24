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
#[cfg(trace_channel)] #[macro_use] extern crate log;
#[cfg(trace_channel)] #[macro_use] extern crate debugit;
extern crate wait_queue;
extern crate mpmc;
extern crate crossbeam_utils;
extern crate core2;

#[cfg(downtime_eval)]
extern crate hpet;
#[cfg(downtime_eval)]
extern crate task;

use alloc::sync::Arc;
use mpmc::Queue as MpmcQueue;
use wait_queue::WaitQueue;
use crossbeam_utils::atomic::AtomicCell;
use core::sync::atomic::{AtomicUsize, Ordering};

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
        channel_status: AtomicCell::new(ChannelStatus::Connected),
        sender_count: AtomicUsize::new(1),
        receiver_count: AtomicUsize::new(1),
    });
    (
        Sender   { channel: channel.clone() },
        Receiver { channel }
    )
}

/// Indicates whether channel is Connected or Disconnected
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChannelStatus {
    /// Channel is working. Initially channel is created with Connected status.
    Connected,
    /// Set to Disconnected when Sender end is dropped.
    SenderDisconnected,
    /// Set to Disconnected when Receiver end is dropped.
    ReceiverDisconnected,
}

/// Error type for tracking different type of errors sender and receiver 
/// can encounter.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// Occurs when a "try" operation would need to block to complete.
    ///
    /// I.e. `try_send` is performed on a full channel or `try_receive` is
    /// performed on an empty channel.
    WouldBlock,
    /// Occurs when one end of channel is dropped
    ChannelDisconnected,
    /// Occurs when an error occur in `WaitQueue`
    WaitError(wait_queue::WaitError)
}

impl From<Error> for core2::io::Error {
    fn from(e: Error) -> Self {
        match e {
            Error::WouldBlock => core2::io::ErrorKind::WouldBlock,
            Error::ChannelDisconnected => core2::io::ErrorKind::BrokenPipe,
            Error::WaitError(_) => core2::io::ErrorKind::Other,
        }
        .into()
    }
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
    channel_status: AtomicCell<ChannelStatus>,
    sender_count: AtomicUsize,
    receiver_count: AtomicUsize,
}

// Ensure that `AtomicCell<ChannelStatus>` is actually a lock-free atomic.
const _: () = assert!(AtomicCell::<ChannelStatus>::is_lock_free());

impl <T: Send> Channel<T> {
    /// Returns true if the channel is disconnected.
    #[inline(always)]
    fn is_disconnected(&self) -> bool {
        self.get_channel_status() != ChannelStatus::Connected
    }

    /// Returns the channel's current status.
    #[inline(always)]
    fn get_channel_status(&self) -> ChannelStatus {
        self.channel_status.load()
    }
    
    /// Returns another `Sender` endpoint connected to the given channel.
    ///
    /// This increments the channel's sender count.
    /// If there were previously no senders, the channel status is updated to `Connected`.
    fn add_sender(channel: &Arc<Channel<T>>) -> Sender<T> {
        if channel.sender_count.fetch_add(1, Ordering::SeqCst) == 0 {
            channel.channel_status.store(ChannelStatus::Connected);
        }
        Sender { channel: channel.clone() }
    }
    
    /// Returns another `Receiver` endpoint connected to the given channel.
    ///
    /// This increments the channel's receiver count.
    /// If there were previously no receivers, the channel status is updated to `Connected`.
    fn add_receiver(channel: &Arc<Channel<T>>) -> Receiver<T> {
        if channel.receiver_count.fetch_add(1, Ordering::SeqCst) == 0 {
            channel.channel_status.store(ChannelStatus::Connected);
        }
        Receiver { channel: channel.clone() }
    }
}

/// The sender (transmit) side of a channel.
pub struct Sender<T: Send> {
    channel: Arc<Channel<T>>,
}

impl<T:Send> Clone for Sender<T> {
    /// Clones this `Sender`, returning another `Sender` connected to the same channel.
    ///
    /// This increments the channel's sender count.
    /// If there were previously no senders, the channel status is updated to `Connected`.
    fn clone(&self) -> Self {
        Channel::add_sender( &self.channel )
    }
}

impl <T: Send> Sender<T> {
    /// Send a message, blocking until space in the channel's buffer is available. 
    /// 
    /// Returns `Ok(())` if the message was sent successfully,
    /// otherwise returns an [`Error`]. 
    pub fn send(&self, msg: T) -> Result<(), Error> {
        #[cfg(trace_channel)]
        trace!("async_channel: sending msg: {:?}", debugit!(msg));
        // Fast path: attempt to send the message, assuming the buffer isn't full
        let msg = match self.try_send(msg) {
            // if successful return ok
            Ok(()) => return Ok(()),
            // if unsunccessful check whether it fails due to any other reason than channel being full
            Err((returned_msg, channel_error)) => {
                if channel_error != Error::WouldBlock {
                    return Err(channel_error);
                }
                returned_msg
            },
        };

        // Slow path: the buffer was full, so now we need to block until space becomes available.
        // The code can move to this point only if fast path failed due to channel being full
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
                    // We wrap the result in Some() since `wait_until` progresses only when `Some` is returned.
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

            if self.channel.is_disconnected() {
                 // trace!("Receiver Endpoint is dropped");
                 // Here the receiver end has dropped. 
                 // So we don't wait anymore in the waitqueue
                 Some(Err(Error::ChannelDisconnected))
            } else {
                result
            }
            
        };

        // When `wait_until_mut` returns it can be either a successful send marked as  Ok(Ok()), 
        // Error in the condition (channel disconnection) marked as Ok(Err()),
        // or the wait_until runs into error (Err()) 
        let res =  match self.channel.waiting_senders.wait_until_mut(&mut closure) {
            Ok(r) => r,
            Err(wait_error) => Err(Error::WaitError(wait_error)),
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

    /// Sends a slice of objects through the channel, returning how many objects were sent.
    ///
    /// This method only blocks on the first object being sent.
    pub fn send_buf(&self, buf: &[T]) -> Result<usize, Error>
    where
        T: Copy
    {
        for (idx, item) in buf.iter().enumerate() {
            if idx == 0 {
                self.send(*item)?;
            } else {
                match self.try_send(*item) {
                    Ok(_) => {},
                    Err((_, Error::WouldBlock)) => return Ok(idx),
                    Err((_, e)) => return Err(e),
                }
            }
        }
        Ok(buf.len())
    }

    /// Attempts to send an entire slice of objects through the channel.
    pub fn send_all(&self, buf: &[T]) -> Result<(), Error>
    where
        T: Copy
    {
        for item in buf.iter() {
            self.send(*item)?;
        }
        Ok(())
    }

    /// Tries to send the message, only succeeding if buffer space is available.
    /// 
    /// If no buffer space is available, it returns the `msg`  with `Error` back to the caller without blocking. 
    pub fn try_send(&self, msg: T) -> Result<(), (T, Error)> {
        // first we'll check whether the channel is active
        match self.channel.get_channel_status() {
            ChannelStatus::SenderDisconnected => {
                self.channel.channel_status.store(ChannelStatus::Connected);
            },
            ChannelStatus::ReceiverDisconnected  => {
                return Err((msg, Error::ChannelDisconnected));
            },
            _ => {},
        }

        // Injected Randomized fault : Page fault
        #[cfg(downtime_eval)]
        {
            let value = hpet::get_hpet().as_ref().unwrap().get_counter();
            // debug!("Value {} {}", value, value % 1024);

            let is_restartable = task::with_current_task(|t| t.is_restartable()).unwrap_or(false);
            // We restrict the fault to a specific task to make measurements consistent
            if (value % 4096) == 0  && is_restartable {
                // debug!("Fake error {}", value);
                unsafe { *(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555; }
            }
        }

        match self.channel.queue.push(msg) {
            // successfully sent
            Ok(()) => {
                // trace!("successful try_send() is notifying receivers.");
                self.channel.waiting_receivers.notify_one();
                Ok(())
            }
            // queue was full, return message back to caller
            Err(returned_msg) => Err((returned_msg, Error::WouldBlock)),
        }
    }

    /// Sends a slice of objects through the channel, returning how many objects were sent.
    ///
    /// This method does not block.
    pub fn try_send_buf(&self, buf: &[T]) -> Result<usize, Error>
    where
        T: Copy
    {
        for (idx, item) in buf.iter().enumerate() {
            if idx == 0 {
                self.try_send(*item).map_err(|(_, e)| e)?;
            } else {
                match self.try_send(*item) {
                    Ok(_) => {},
                    Err((_, Error::WouldBlock)) => return Ok(idx),
                    Err((_, e)) => return Err(e),
                }
            }
        }
        Ok(buf.len())
    }

    /// Returns true if the channel is disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.channel.is_disconnected()
    }

    /// Obtain a `Receiver` endpoint connected to the same channel as this `Sender`.
    pub fn receiver(&self) -> Receiver<T> {
        Channel::add_receiver( &self.channel )
    }
}

/// The receiver side of a channel.
pub struct Receiver<T: Send> {
    channel: Arc<Channel<T>>,
}

impl<T: Send> Clone for Receiver<T> {
    /// Clones this `Receiver`, returning another `Receiver` connected to the same channel.
    ///
    /// This increments the channel's receiver count.
    /// If there were previously no receivers, the channel status is updated to `Connected`.
    fn clone(&self) -> Self {
        Channel::add_receiver( &self.channel )
    }
}

impl <T: Send> Receiver<T> {
    /// Receive a message, blocking until a message is available in the buffer.
    /// 
    /// Returns the message if it was received properly, otherwise returns an [`Error`].
    pub fn receive(&self) -> Result<T, Error> {
        // trace!("async_channel: receive() entry");
        // Fast path: attempt to receive a message, assuming the buffer isn't empty
        // The code progresses beyond this match only if try_receive fails due to
        // empty channel
        match self.try_receive() {
            Err(Error::WouldBlock) => {},
            x => {
                #[cfg(trace_channel)]
                trace!("async_channel: received msg: {:?}", debugit!(x));
                return x;
            }
        };

        // Slow path: the buffer was empty, so we need to block until a message is sent.
        // trace!("waiting to receive a message...");
        
        // This closure is invoked from within a locked context, so we cannot just call `try_receive()` here
        // because it will notify the receivers which can cause deadlock.
        // Therefore, we need to perform the nofity action outside of this closure after it returns
        // Closure would output the message if received or an error if channel is disconnected.
        // It would output `None` if neither happens, resulting in waiting in the queue. 
        let closure = || {
            match self.channel.queue.pop() {
                Some(msg) => Some(Ok(msg)),
                _ => {
                    if self.channel.is_disconnected() {
                        Some(Err(Error::ChannelDisconnected))
                    } else {
                        None
                    }
                },
            }
        };

        // When wait returns it can be either a successful receiver marked as  Ok(Ok(msg)), 
        // Error in wait condition marked as Ok(Err(error)),
        // or the wait_until runs into error (Err()) 
        let res =  match self.channel.waiting_receivers.wait_until(& closure) {
            Ok(Ok(x)) => Ok(x),
            Ok(Err(error)) => Err(error),
            Err(wait_error) => Err(Error::WaitError(wait_error)),
        };

        // trace!("... received msg.");

        // If we successfully received a message, we need to notify any waiting senders.
        // As stated above, to avoid deadlock, this must be done here rather than in the above closure.
        if let Ok(ref _msg) = res {
            // trace!("async_channel: successful receive() is notifying senders.");
            self.channel.waiting_senders.notify_one();
        }

        #[cfg(trace_channel)]
        trace!("async_channel: received msg: {:?}", debugit!(res));
        
        res
    }

    /// Receives objects placing them in a buffer and returning the number of objects received.
    ///
    /// This method only blocks on the first object being received.
    pub fn receive_buf(&self, buf: &mut [T]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }

        let mut byte = self.receive()?;
        let mut read = 0;

        loop {
            buf[read] = byte;
            read += 1;

            if read == buf.len() {
                return Ok(read);
            }

            byte = match self.try_receive() {
                Ok(b) => b,
                Err(Error::WouldBlock) => return Ok(read),
                Err(e) => return Err(e),
            };
        }
    }

    /// Tries to receive a message, only succeeding if a message is already available in the buffer.
    /// 
    /// If receive succeeds returns `Some(Ok(T))`. 
    /// If an endpoint is disconnected returns `Some(Err(ChannelStatus::Disconnected))`. 
    /// If no such message exists, it returns `None` without blocking
    pub fn try_receive(&self) -> Result<T, Error> {
        if let Some(msg) = self.channel.queue.pop() {
            // trace!("successful try_receive() is notifying senders.");
            self.channel.waiting_senders.notify_one();
            Ok(msg)
        } else {
            // We check whether the channel is disconnected
            match self.channel.get_channel_status() {
                ChannelStatus::ReceiverDisconnected => {
                    self.channel.channel_status.store(ChannelStatus::Connected);
                    Err(Error::WouldBlock)
                },
                ChannelStatus::SenderDisconnected  => {
                    Err(Error::ChannelDisconnected)
                },
                _ => {
                    Err(Error::WouldBlock)
                },
            }
        }
    }

    /// Receives objects placing them in a buffer and returning the number of objects received.
    ///
    /// This method does not block.
    pub fn try_receive_buf(&self, buf: &mut [T]) -> Result<usize, Error> {
        for (idx, item) in buf.iter_mut().enumerate() {
            *item = match self.try_receive() {
                Ok(byte) => byte,
                Err(Error::WouldBlock) => return Ok(idx + 1),
                Err(e) => return Err(e),
            };
        }
        Ok(buf.len())
    }

    /// Returns true if the channel is disconnected.
    pub fn is_disconnected(&self) -> bool {
        self.channel.is_disconnected()
    }

    /// Obtain a `Sender` endpoint connected to the same channel as this `Receiver`.
    pub fn sender(&self) -> Sender<T> {
        Channel::add_sender( &self.channel )
    }
}


/// When the only remaining `Receiver` is dropped, we mark the channel as disconnected
/// and notify all of the `Senders`
impl<T: Send> Drop for Receiver<T> {
    fn drop(&mut self) {
        // trace!("Dropping a receiver");
        if self.channel.receiver_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.channel.channel_status.store(ChannelStatus::ReceiverDisconnected);
            self.channel.waiting_senders.notify_all();
        }
    }
}

/// When the only remaining `Sender` is dropped, we mark the channel as disconnected
/// and notify all of the `Receivers`
impl<T: Send> Drop for Sender<T> {
    fn drop(&mut self) {
        // trace!("Dropping a sender");
        if self.channel.sender_count.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.channel.channel_status.store(ChannelStatus::SenderDisconnected);
            self.channel.waiting_receivers.notify_all();
        }
    }
}
