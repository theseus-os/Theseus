//! An implementation of a shared buffer for IPC
//! Still need to add the Sender and Receiver structs for safety, and make the mutex based channel generic

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;
extern crate atomic;
extern crate bit_field;

use core::sync::atomic::{Ordering, AtomicBool, AtomicU8, AtomicU16, spin_loop_hint};
use alloc::sync::Arc;
use spin::Mutex;
use atomic::Atomic;
use bit_field::BitField;

pub struct Channel {
    // the upper 8 bits are the buffer and lower 8 bits are flags
    // specifically bit 0 tells us if the buffer has been used or not
    buffer: AtomicU16,
}

impl Channel {
    pub fn new() -> Channel {
        Channel{
            buffer: AtomicU16::new(0),
        }
    }
    pub fn send(&self, msg: u8) {
        let msg: u16 = ((msg as u16) << 8) | 0x1;
        while self.buffer.load(Ordering::SeqCst).get_bit(0) {
            spin_loop_hint(); // doesn't really make any difference in performance
        }
        self.buffer.store(msg, Ordering::SeqCst);
    }

    pub fn receive(&self) -> u8 {
        while !self.buffer.load(Ordering::SeqCst).get_bit(0) {
            spin_loop_hint();
        }
        let msg = (self.buffer.load(Ordering::SeqCst) >> 8) & 0xFF;
        self.buffer.store(0, Ordering::SeqCst);
        msg as u8
    }
}

pub fn new_channel() -> (Arc<Channel>, Arc<Channel>) {
    let sender = Arc::new(Channel::new());
    let receiver = sender.clone();
    (sender, receiver)
}

// pub struct Channel {
//     buffer: Mutex<u8>,
//     used: AtomicBool,
// }

// impl Channel {
//     pub fn new() -> Channel {
//         Channel{
//             buffer: Mutex::new(0),
//             used: AtomicBool::new(false)
//         }
//     }
//     pub fn send(&self, msg: u8) {
//         while self.used.load(Ordering::SeqCst) == true {}
//         *self.buffer.lock() = msg;
//         self.used.store(true, Ordering::SeqCst);

//     }

//     pub fn receive(&self) -> u8 {
//         while self.used.load(Ordering::SeqCst) == false {}
//         let msg = *self.buffer.lock();
//         self.used.store(false, Ordering::SeqCst);
//         msg
//     }
// }


// pub struct Channel {
//     buffer: AtomicU8,
//     used: AtomicBool,
// }

// impl Channel {
//     pub fn new() -> Channel {
//         Channel{
//             buffer: AtomicU8::new(0),
//             used: AtomicBool::new(false)
//         }
//     }
//     pub fn send(&self, msg: u8) {
//         while self.used.load(Ordering::SeqCst) == true {
//             spin_loop_hint(); // doesn't really make any difference in performance
//         }
//         self.buffer.store(msg, Ordering::SeqCst);
//         self.used.store(true, Ordering::SeqCst);

//     }

//     pub fn receive(&self) -> u8 {
//         while self.used.load(Ordering::SeqCst) == false {
//             spin_loop_hint();
//         }
//         let msg = self.buffer.load(Ordering::SeqCst);
//         self.used.store(false, Ordering::SeqCst);
//         msg
//     }
// }
