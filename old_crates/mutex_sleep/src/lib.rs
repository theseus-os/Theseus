//! Locking types that put a task to sleep while it waits for the lock. 
//! 
//! These are Theseus-specific locking types that ensure mutual exclusion
//! using [`spin::Mutex`] and [`spin::RwLock`] under the hood;
//! see those types for more details on how they work.

#![no_std]

mod mutex;
mod rwlock;

pub use mutex::*;
pub use rwlock::*;
