#![no_std]

extern crate alloc;
extern crate heap;
extern crate irq_safety;
extern crate kernel_config;
#[macro_use] extern crate log;
extern crate memory;
extern crate multiboot2;

use memory::{MappedPages, MemoryManagementInfo, VirtualAddress};
use kernel_config::memory::{KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use irq_safety::MutexIrqSafe;
use multiboot2::BootInformation;
use alloc::{ 
    vec::Vec,
    sync::Arc
};
use heap::HEAP_FLAGS;
use core::ops::DerefMut;

/// Just like Rust's `try!()` macro, 
/// but forgets the given `obj`s to prevent them from being dropped,
/// as they would normally be upon return of an Error using `try!()`.
/// This must come BEFORE the below modules in order for them to be able to use it.
macro_rules! try_forget {
    ($expr:expr, $($obj:expr),*) => (match $expr {
        Ok(val) => val,
        Err(err) => {
            $(
                core::mem::forget($obj);
            )*
            return Err(err);
        }
    });
}

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
///  * the kernel's list of identity-mapped MappedPages which should be dropped before starting the first user application. 
pub fn init_memory_management(boot_info: &BootInformation)  
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str>
{
    memory::dump_page_allocator_state();

    // Initialize memory management: paging (create a new page table), essential kernel mappings
    let (allocator_mutex, mut page_table, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages) = memory::init(&boot_info)?;

    memory::dump_page_allocator_state();

    // Initialize the kernel heap.
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop. 
    // We use the try_forget!() macro to do so.
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;
    
    let heap_mapped_pages = {
        let pages = memory::allocate_pages_by_bytes_at(VirtualAddress::new_canonical(heap_start), heap_initial_size)?;
        let mut allocator = allocator_mutex.lock();
        let heap_mp = try_forget!(
            page_table.map_allocated_pages(pages, HEAP_FLAGS, allocator.deref_mut())
                .map_err(|e| {
                    error!("Failed to map kernel heap memory pages, {} bytes starting at virtual address {:#X}. Error: {:?}", KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, e);
                    "Failed to map the kernel heap memory. Perhaps the KERNEL_HEAP_INITIAL_SIZE exceeds the size of the system's physical memory?"
                }),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages
        );
        heap::init_single_heap(heap_start, heap_initial_size);
        heap_mp
    };

    memory::dump_page_allocator_state();
    
    debug!("mapped and initialized the initial heap");

    // Initialize memory management post heap intialization: set up kernel stack allocator and kernel memory management info.
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init_post_heap(page_table, higher_half_mapped_pages, identity_mapped_pages, heap_mapped_pages)?;

    Ok((kernel_mmi_ref, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages))
}


