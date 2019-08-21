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

extern crate memory_x86;
extern crate memory;
extern crate alloc;
extern crate multiboot2;
extern crate irq_safety;

use memory_x86::{arch_init, BootInformation};
pub use memory::*;

use alloc::vec::Vec;
use irq_safety::MutexIrqSafe;
use alloc::sync::Arc;

pub fn init(boot_info: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    arch_init(boot_info)
}