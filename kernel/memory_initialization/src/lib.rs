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

/// Initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents the initial (kernel) address space. 
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  * The kernel's new MemoryManagementInfo
///  * the MappedPages of the kernel's text section,
///  * the MappedPages of the kernel's rodata section,
///  * the MappedPages of the kernel's data section,
///  * the initial stack for this CPU (e.g., the BSP stack) that is currently in use,
///  * the kernel's list of identity-mapped MappedPages which should be dropped before starting the first user application. 
pub fn init_memory_management(boot_info: &BootInformation)  
    -> Result<(MmiRef, MappedPages, MappedPages, MappedPages, Stack, Vec<MappedPages>), &'static str>
{
    x86_64::init_memory_management(boot_info)
}

}
else if #[cfg(target_arch="arm")] {

extern crate alloc;
extern crate alloc_cortex_m;
extern crate cortex_m_rt;
extern crate kernel_config;

mod armv7em;

pub fn init_memory_management() {
    armv7em::init_memory_management()
}

}
}
