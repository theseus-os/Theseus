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

/// This function is an abstract interface. In invokes arch-specific init functions to initialize the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  * The kernel's new MemoryManagementInfo
///  * the MappedPages of the kernel's text section,
///  * the MappedPages of the kernel's rodata section,
///  * the MappedPages of the kernel's data section,
///  * the kernel's list of *other* higher-half MappedPages, which should be kept forever. 
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
