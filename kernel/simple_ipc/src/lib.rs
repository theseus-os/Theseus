//! An implementation of a shared buffer for IPC that can be used for 1-byte messages.
//! We still need to make the channel generic to use atomics upto AtomicU64

#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
extern crate bit_field;

use core::sync::atomic::{Ordering, AtomicU16};
use alloc::sync::Arc;
use bit_field::BitField;

/// A channel implemented using a lock-free shared buffer 
struct Channel {
    // The upper 8 bits are the buffer and the LSB is the full flag which indicates
    // whether the buffer has been used and has a message stored in it. 
    buffer: AtomicU16,
}

impl Channel {
    pub fn new() -> Channel {
        Channel{
            buffer: AtomicU16::new(0),
        }
    }
}

/// Channel endpoint that only allows sending messages.
pub struct Sender(Arc<Channel>);

impl Sender{

    /// Tries to send a message once. If the buffer is full, then returns an Err.
    pub fn try_send(&self, msg: u8) -> Result<(), &'static str> {
        self.0.buffer.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |val| {
                if !val.get_bit(0) {
                    let msg: u16 = ((msg as u16) << 8) | 0x1;
                    Some(msg)
                } else {
                    None
                }
            }
        )
        .map(|_prev_val| ())
        .map_err(|_e| "Buffer has reached its capacity")
    }

    /// Tries to send a message until succesful.
    /// Task will spin in a loop until the full flag is cleared. 
    pub fn send(&self, msg: u8) {
        let mut res = self.try_send(msg);
        while res.is_err() {
            res = self.try_send(msg);
        }
    }
}

/// Channel endpoint that only allows receiving messages.
pub struct Receiver(Arc<Channel>);

impl Receiver {

    /// Tries to receive a message once. If the buffer is empty, then returns an Err.
    pub fn try_receive(&self) -> Result<u8, &'static str> {
        self.0.buffer.fetch_update(
            Ordering::SeqCst,
            Ordering::SeqCst,
            |val| {
                if val.get_bit(0) {
                    Some(0)
                } else {
                    None
                }
            }
        )
        .map(|msg| (msg >> 8) as u8)
        .map_err(|_e| "There was no message in the buffer.")
    }

    /// Tries to receive a message until succesful.
    /// Task will spin in a loop until the full flag is set.
    pub fn receive(&self) -> u8 {
        let mut res = self.try_receive();
        while res.is_err() {
            res = self.try_receive();
        }
        // unwrap is safe here since the condition is checked in the loop
        res.unwrap()
    }
}

/// Creates a new channel and returns the endpoints
pub fn new_channel() -> (Sender, Receiver) {
    let sender = Arc::new(Channel::new());
    let receiver = sender.clone();
    (Sender(sender), Receiver(receiver))
}
