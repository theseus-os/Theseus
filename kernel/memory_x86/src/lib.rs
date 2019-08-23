//! This crate implements the virtual memory subsystem interfaces for Theseus on x86_64. 
//! `memory_interface` uses this crate to manipulate the memory subsystem on x86_64;

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate x86_64;
extern crate page_table_x86;

mod paging;

/// Export arch-specific information structure to `memory`. 
pub use multiboot2::BootInformation;
/// This is top arch-specific memory crate. Export all memory-related definitions here.
pub use page_table_x86::*;

use core::ops::DerefMut;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{};


