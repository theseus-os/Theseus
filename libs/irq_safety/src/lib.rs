//! Irq-safe locking and 
//! a high level Rust wrapper.

#![feature(asm, const_fn)]
#![feature(manually_drop)]
#![no_std]

extern crate x86;
extern crate spin;

pub use rwlock_irqsafe::*;
pub use held_interrupts::*;

// pub mod mutex_irqsafe; // TODO
mod rwlock_irqsafe;
mod held_interrupts;
