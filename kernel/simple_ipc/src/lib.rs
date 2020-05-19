//! An implementation of a shared buffer for IPC
//! Still need to add the Sender and Receiver structs for safety, and make the mutex based channel generic

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate spin;

use core::sync::atomic::{Ordering, AtomicBool, AtomicU8};
use alloc::sync::Arc;
use spin::Mutex;


pub struct Channel {
    buffer: AtomicU8,
    used: AtomicBool,
}

impl Channel {
    pub fn new() -> Channel {
        Channel{
            buffer: AtomicU8::new(0),
            used: AtomicBool::new(false)
        }
    }
    pub fn send(&self, msg: u8) {
        while self.used.load(Ordering::SeqCst) == true {}
        self.buffer.store(msg, Ordering::SeqCst);
        self.used.store(true, Ordering::SeqCst);

    }

    pub fn receive(&self) -> u8 {
        while self.used.load(Ordering::SeqCst) == false {}
        let msg = self.buffer.load(Ordering::SeqCst);
        self.used.store(false, Ordering::SeqCst);
        msg
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
