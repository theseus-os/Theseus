#![feature(must_not_suspend, negative_impls)]
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]
#![no_std]

mod mutex;
mod rwlock;
mod condvar;

pub use mutex::*;
pub use rwlock::*;
pub use condvar::*;
