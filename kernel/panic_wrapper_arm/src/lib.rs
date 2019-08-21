//! Provides types and simple routines for handling panics.
//! This is similar to what's found in Rust's `core::panic` crate, 
//! but is much simpler and doesn't require lifetimes 
//! (although it does require alloc types like String).
//! 
#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate memory;
extern crate task;
extern crate runqueue;

use core::panic::PanicInfo;
use alloc::string::String;
use task::{KillReason, PanicInfoOwned};

/// TODO: handle panic
pub fn panic_wrapper(panic_info: &PanicInfo) -> Result<(), &'static str> {
    // TODO
    Ok(())
}