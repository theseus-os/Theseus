//! An implementation of a shared buffer for IPC that can be used for 1-byte messages.
//! We still need to make the channel generic to use atomics upto AtomicU64

#![no_std]

extern crate alloc;
// #[macro_use] extern crate log;
extern crate bit_field;

use core::sync::atomic::{Ordering, AtomicU16, spin_loop_hint};
use alloc::sync::Arc;
use bit_field::BitField;

/// A channel implemented using a shared buffer that is represented by an atomic number 
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
        if !self.0.buffer.load(Ordering::SeqCst).get_bit(0) {
            let msg: u16 = ((msg as u16) << 8) | 0x1;
            self.0.buffer.store(msg, Ordering::SeqCst);
            Ok(())
        } else {
            Err("Buffer has reached its capacity")
        }

    }

    /// Tries to send a message until succesful.
    /// Task will spin in a loop until the full flag is cleared. 
    pub fn send(&self, msg: u8) {
        let msg: u16 = ((msg as u16) << 8) | 0x1;
        while self.0.buffer.load(Ordering::SeqCst).get_bit(0) {
            spin_loop_hint(); // doesn't really make any difference in performance
        }
        self.0.buffer.store(msg, Ordering::SeqCst);
    }
}

/// Channel endpoint that only allows receiving messages.
pub struct Receiver(Arc<Channel>);

impl Receiver {

    /// Tries to receive a message once. If the buffer is empty, then returns an Err.
    pub fn try_receive(&self) -> Result<u8, &'static str> {
        if self.0.buffer.load(Ordering::SeqCst).get_bit(0) {
            let msg = (self.0.buffer.load(Ordering::SeqCst) >> 8) & 0xFF;
            self.0.buffer.store(0, Ordering::SeqCst);
            Ok(msg as u8)
        } else {
            Err("There was no message in the buffer")
        }
    }

    /// Tries to receive a message until succesful.
    /// Task will spin in a loop until the full flag is set.
    pub fn receive(&self) -> u8 {
        while !self.0.buffer.load(Ordering::SeqCst).get_bit(0) {
            spin_loop_hint();
        }
        let msg = (self.0.buffer.load(Ordering::SeqCst) >> 8) & 0xFF;
        self.0.buffer.store(0, Ordering::SeqCst);
        msg as u8
    }
}

/// Creates a new channel and returns the endpoints
pub fn new_channel() -> (Sender, Receiver) {
    let sender = Arc::new(Channel::new());
    let receiver = sender.clone();
    (Sender(sender), Receiver(receiver))
}
