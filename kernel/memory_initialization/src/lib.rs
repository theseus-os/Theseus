#![no_std]

extern crate alloc;
extern crate heap;
extern crate kernel_config;
#[macro_use] extern crate log;
extern crate memory;
extern crate stack;
extern crate multiboot2;
extern crate bootloader_modules;

use memory::{MmiRef, MappedPages, VirtualAddress, PhysicalAddress};
use kernel_config::memory::{KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use multiboot2::BootInformation;
use alloc::{
    string::String, 
    vec::Vec,
};
use heap::HEAP_FLAGS;
use stack::Stack;
use bootloader_modules::BootloaderModule;

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
    // After this point, at which `memory::init()` has returned new `MappedPages` that represent
    // the currently-executing code/data regions, we must "forget" all of the above mapped_pages instances
    // if an error occurs, because they will be auto-unmapped from the new page table when they are dropped upon return,
    // causing all execution to stop and a processor fault/reset. 
    // We use the try_forget!() macro to do so.
    let stack = match Stack::from_pages(stack_guard_page, stack_pages) {
        Ok(s) => s,
        Err((_stack_guard_page, stack_pages)) => try_forget!(
            Err("initial Stack was not contiguous in virtual memory"),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, stack_pages, higher_half_mapped_pages, identity_mapped_pages
        )
    };

    // Initialize the kernel heap.
    let heap_start = KERNEL_HEAP_START;
    let heap_initial_size = KERNEL_HEAP_INITIAL_SIZE;
    
    let heap_mapped_pages = {
        let pages = try_forget!(
            memory::allocate_pages_by_bytes_at(VirtualAddress::new_canonical(heap_start), heap_initial_size),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, stack, higher_half_mapped_pages, identity_mapped_pages
        );
        debug!("Initial heap starts at: {:#X}, size: {:#X}, pages: {:?}", heap_start, heap_initial_size, pages);
        let heap_mp = try_forget!(
            page_table.map_allocated_pages(pages, HEAP_FLAGS).map_err(|e| {
                error!("Failed to map kernel heap memory pages, {} bytes starting at virtual address {:#X}. Error: {:?}",
                    KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_START, e
                );
                "Failed to map the kernel heap memory. Perhaps the KERNEL_HEAP_INITIAL_SIZE \
                 exceeds the size of the system's physical memory?"
            }),
            text_mapped_pages, rodata_mapped_pages, data_mapped_pages, stack, higher_half_mapped_pages, identity_mapped_pages
        );
        heap::init_single_heap(heap_start, heap_initial_size);
        heap_mp
    };

    debug!("Mapped and initialized the initial heap");

    // Initialize memory management post heap intialization: set up kernel stack allocator and kernel memory management info.
    let (kernel_mmi_ref, identity_mapped_pages) = memory::init_post_heap(page_table, higher_half_mapped_pages, identity_mapped_pages, heap_mapped_pages)?;

    // Because bootloader modules may overlap with the actual boot information, 
    // we need to preserve those records here in a separate list,
    // such that we can unmap the boot info pages & frames here but still access that info in the future.
    let bootloader_modules: Vec<BootloaderModule> = boot_info.module_tags()
        .map(|m| m.cmdline().map(|module_name| 
            BootloaderModule::new(
                PhysicalAddress::new_canonical(m.start_address() as usize),
                PhysicalAddress::new_canonical(m.end_address()   as usize),
                String::from(module_name),
            )
        ))
        .collect::<Result<Vec<_>, _>>() // collect the `Vec<Result<...>>` into `Result<Vec<...>>`
        .map_err(|_e| "BUG: Bootloader module had invalid non-UTF8 name (cmdline) string")?;

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
