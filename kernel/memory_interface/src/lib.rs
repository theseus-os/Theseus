//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

#[cfg(target_arch = "x86_64")]
extern crate memory_x86;
#[cfg(target_arch = "aarch64")]
extern crate memory_arm;
extern crate memory;
extern crate alloc;
extern crate multiboot2;
extern crate irq_safety;

#[cfg(target_arch = "x86_64")]
use memory_x86::{arch_init, BootInformation};
#[cfg(target_arch = "aarch64")]
use memory_arm::{arch_init, BootInformation};
pub use memory::*;

use alloc::vec::Vec;
use irq_safety::MutexIrqSafe;
use alloc::sync::Arc;

pub fn init(boot_info: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    arch_init(boot_info)
}