#![no_std]

extern crate alloc;
extern crate heap;
extern crate kernel_config;
#[macro_use] extern crate log;
extern crate memory;
extern crate stack;
extern crate no_drop;
extern crate bootloader_modules;
extern crate boot_info;

use memory::{MmiRef, MappedPages, VirtualAddress, PhysicalAddress};
use kernel_config::memory::{KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use alloc::{
    string::String, 
    vec::Vec,
};
use heap::HEAP_FLAGS;
use stack::Stack;
use no_drop::NoDrop;
use bootloader_modules::BootloaderModule;
use boot_info::Module;


/// Initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents the initial (kernel) address space. 
///
/// This consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  1. The kernel's new MemoryManagementInfo,
///  2. the MappedPages of the kernel's text section,
///  3. the MappedPages of the kernel's rodata section,
///  4. the MappedPages of the kernel's data section,
///  5. the initial stack for this CPU (e.g., the BSP stack) that is currently in use,
///  6. the list of bootloader modules obtained from the given `boot_info`,
///  7. the kernel's list of identity-mapped [`MappedPages`],
///     which must not be dropped until all AP (additional CPUs) are fully booted,
///     but *should* be dropped before starting the first user application.
pub fn init_memory_management<T>(
    boot_info: T,
) -> Result<(
        MmiRef,
        NoDrop<MappedPages>,
        NoDrop<MappedPages>,
        NoDrop<MappedPages>,
        NoDrop<Stack>,
        Vec<BootloaderModule>,
        NoDrop<Vec<MappedPages>>,
    ), &'static str>
where
    T: boot_info::BootInformation,
{
    // Initialize memory management: paging (create a new page table), essential kernel mappings
    let (
        mut page_table, 
        text_mapped_pages, 
        rodata_mapped_pages, 
        data_mapped_pages, 
        (stack_guard_page, stack_pages), 
        boot_info_mapped_pages,
        higher_half_mapped_pages, 
        identity_mapped_pages
    ) = memory::init(&boot_info)?;
    // After this point, at which `memory::init()` has returned new objects that represent
    // the currently-executing code/data/stack, we must ensure they aren't dropped if an error occurs,
    // because that will cause them to be auto-unmapped.
    // That will then cause all execution to stop and a processor fault/reset. 
    // We use the `NoDrop` type wrapper to accomplish this.
    let stack = match Stack::from_pages(stack_guard_page, stack_pages.into_inner()) {
        Ok(s) => NoDrop::new(s),
        Err((_stack_guard_page, stack_mp)) => {
            let _stack_mp = NoDrop::new(stack_mp);
            return Err("initial Stack was not contiguous in virtual memory");
        }
    };

    // Initialize the kernel heap.
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;
    
    let heap_mapped_pages = {
        let pages = memory::allocate_pages_by_bytes_at(VirtualAddress::new_canonical(heap_start), heap_initial_size)?;
        debug!("Initial heap starts at: {:#X}, size: {:#X}, pages: {:?}", heap_start, heap_initial_size, pages);
        let heap_mp = page_table.map_allocated_pages(pages, HEAP_FLAGS).map_err(|e| {
            error!("Failed to map kernel heap memory pages, {} bytes starting at virtual address {:#X}. Error: {:?}",
                KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, e
            );
            "Failed to map the kernel heap memory. Perhaps the KERNEL_HEAP_INITIAL_SIZE \
                exceeds the size of the system's physical memory?"
        })?;
        heap::init_single_heap(heap_start, heap_initial_size);
        heap_mp
    };

    debug!("Mapped and initialized the initial heap");

    // Initialize memory management post heap intialization: set up kernel stack allocator and kernel memory management info.
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init_post_heap(
        page_table,
        higher_half_mapped_pages,
        identity_mapped_pages,
        heap_mapped_pages,
    );

    // Because bootloader modules may overlap with the actual boot information, 
    // we need to preserve those records here in a separate list,
    // such that we can unmap the boot info pages & frames here but still access that info in the future.
    let bootloader_modules: Vec<BootloaderModule> = boot_info.modules()
        .map(|m| m.name().map(|module_name| {
            BootloaderModule::new(
                PhysicalAddress::new_canonical(m.start() as usize),
                PhysicalAddress::new_canonical(m.end()   as usize),
                String::from(module_name),
            )
        }))
        .collect::<Result<Vec<_>, _>>()?; // collect the `Vec<Result<...>>` into `Result<Vec<...>>`

    // Now that we've recorded the rest of the necessary boot info, we can drop the boot_info_mapped_pages.
    // This frees up those frames such that future code can exclusively map and access those pages/frames.
    drop(boot_info_mapped_pages);

    Ok((
        kernel_mmi_ref,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        stack,
        bootloader_modules,
        identity_mapped_pages
    ))
}
