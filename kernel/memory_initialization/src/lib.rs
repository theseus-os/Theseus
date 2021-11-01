#![no_std]
#![feature(alloc_error_handler)]

extern crate cfg_if;

cfg_if::cfg_if! {
if #[cfg(target_arch="x86_64")] {

extern crate alloc;
extern crate heap;
extern crate kernel_config;
#[macro_use] extern crate log;
extern crate memory;
extern crate stack;
extern crate multiboot2;

mod x86_64;

use memory::{MmiRef, MappedPages};
use multiboot2::BootInformation;
use alloc::vec::Vec;
use stack::Stack;
pub use x86_64::BootloaderModule;

/// Initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents the initial (kernel) address space. 
///
/// This consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  1. The kernel's new MemoryManagementInfo
///  2. the MappedPages of the kernel's text section,
///  3. the MappedPages of the kernel's rodata section,
///  4. the MappedPages of the kernel's data section,
///  5. the initial stack for this CPU (e.g., the BSP stack) that is currently in use,
///  6. the list of bootloader modules obtained from the given `boot_info`,
///  7. the kernel's list of identity-mapped MappedPages which should be dropped before starting the first user application. 
pub fn init_memory_management(
    boot_info: BootInformation
) -> Result<(
        MmiRef,
        MappedPages,
        MappedPages,
        MappedPages,
        Stack,
        Vec<BootloaderModule>,
        Vec<MappedPages>
    ), &'static str>
{
    x86_64::init_memory_management(boot_info)
}

}
else if #[cfg(target_arch="arm")] {

extern crate alloc;
extern crate alloc_cortex_m;
extern crate page_allocator;
extern crate memory_structs;

mod armv7em;

pub fn init_memory_management(heap_start: usize, heap_end: usize) {
    armv7em::init_memory_management(heap_start, heap_end)
}

}
}
