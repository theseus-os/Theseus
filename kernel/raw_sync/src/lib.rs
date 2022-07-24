#![feature(result_option_inspect)]
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
#![no_std]

extern crate alloc;

mod condvar;
mod mutex;
mod rwlock;

pub use condvar::Condvar;
pub use mutex::Mutex;
pub use rwlock::RwLock;
