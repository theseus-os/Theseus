//! This crate provides abstract interfaces for memory subsytem. 
//! It invokes arch-specific functions to manipulate the memory subsystem.

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]
#![feature(step_trait, range_is_empty)]

extern crate alloc;
extern crate irq_safety;
extern crate memory;
extern crate memory_x86;

use memory::{MappedPages, MemoryManagementInfo};
use memory_x86::{arch_init, BootInformation};

use alloc::sync::Arc;
use alloc::vec::Vec;
use irq_safety::MutexIrqSafe;

/// Initiialize the memory subsystem. 
/// This function is an abstract interface. In invokes arch-specific init functions to initialize the memory subsystem.
pub fn init(
    boot_info: &BootInformation,
) -> Result<
    (
        Arc<MutexIrqSafe<MemoryManagementInfo>>,
        MappedPages,
        MappedPages,
        MappedPages,
        Vec<MappedPages>,
    ),
    &'static str,
> {
    arch_init(boot_info)
}
