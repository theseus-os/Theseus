//! Irq-safe locking and 
//! a high level Rust wrapper.

#![cfg_attr(feature = "const_fn", feature(const_fn))]

#![feature(asm)]
#![feature(manually_drop)]
#![no_std]

extern crate x86;
extern crate spin;

pub use mutex_irqsafe::*;
pub use rwlock_irqsafe::*;
pub use held_interrupts::*;

mod mutex_irqsafe;
mod rwlock_irqsafe;
mod held_interrupts;
