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

extern crate spin;
extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate xmas_elf;
#[cfg(target_arch = "x86_64")]
extern crate x86_64;
#[cfg(any(target_arch = "aarch64"))]
extern crate aarch64;
#[macro_use] extern crate bitflags;
extern crate heap_irq_safe;
#[macro_use] extern crate derive_more;
extern crate bit_field;
extern crate type_name;
extern crate uefi;
extern crate memory;

pub use memory::*;
pub use multiboot2::BootInformation;

use core::{
    ops::{RangeInclusive, Deref, DerefMut},
    iter::Step,
    mem,
};

use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR, ENTRIES_PER_PAGE_TABLE};
#[cfg(target_arch = "x86_64")]
use kernel_config::memory::x86_64::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX};
#[cfg(any(target_arch = "aarch64"))]
use kernel_config::memory::arm::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, HARDWARE_START, HARDWARE_END};
use bit_field::BitField;
use uefi::prelude::*;
use uefi::table::boot::{MemoryDescriptor, MemoryType};


/// Initializes the virtual memory management system and returns a MemoryManagementInfo instance,
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
#[cfg(target_arch = "x86_64")]
pub fn arch_init(boot_info: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    let memory_map_tag = boot_info.memory_map_tag().ok_or("Memory map tag not found")?;
    let elf_sections_tag = boot_info.elf_sections_tag().ok_or("Elf sections tag not found")?;

    // Our linker script specifies that the kernel will have the .init section starting at 1MB and ending at 1MB + .init size
    // and all other kernel sections will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // Thus, kernel_phys_start is the same as kernel_virt_start initially, but we remap them later in paging::init.
    let kernel_phys_start = PhysicalAddress::new(
        elf_sections_tag.sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.start_address())
            .min()
            .ok_or("Couldn't find kernel start (phys) address")? as usize
    )?;
    let kernel_virt_end = VirtualAddress::new(
        elf_sections_tag.sections()
            .filter(|s| s.is_allocated())
            .map(|s| s.end_address())
            .max()
            .ok_or("Couldn't find kernel end (virt) address")? as usize
    )?;
    let kernel_phys_end = PhysicalAddress::new(kernel_virt_end.value() - KERNEL_OFFSET)?;

    debug!("kernel_phys_start: {:#x}, kernel_phys_end: {:#x} kernel_virt_end = {:#x}",
        kernel_phys_start,
        kernel_phys_end,
        kernel_virt_end
    );
  
    // parse the list of physical memory areas from multiboot
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
    for area in memory_map_tag.memory_areas() {
        let area_start = PhysicalAddress::new(area.start_address() as usize)?;
        let area_end   = PhysicalAddress::new(area.end_address() as usize)?;
        let area_size  = area.size() as usize;
        debug!("memory area base_addr={:#x} length={:#x} ({:?})", area_start, area_size, area);
        
        // optimization: we reserve memory from areas below the end of the kernel's physical address,
        // which includes addresses beneath 1 MB
        if area_end < kernel_phys_end {
            debug!("--> skipping region before kernel_phys_end");
            continue;
        }
        let start_paddr: PhysicalAddress = if area_start >= kernel_phys_end { area_start } else { kernel_phys_end };
        let start_paddr = (Frame::containing_address(start_paddr) + 1).start_address(); // align up to next page

        available[avail_index] = PhysicalMemoryArea {
            base_addr: start_paddr,
            size_in_bytes: area_size,
            typ: 1, 
            acpi: 0, 
        };

        info!("--> memory region established: start={:#x}, size_in_bytes={:#x}", available[avail_index].base_addr, available[avail_index].size_in_bytes);
        // print_early!("--> memory region established: start={:#x}, size_in_bytes={:#x}\n", available[avail_index].base_addr, available[avail_index].size_in_bytes);
        avail_index += 1;
    }

    // calculate the bounds of physical memory that is occupied by modules we've loaded 
    // (we can reclaim this later after the module is loaded, but not until then)
    let (modules_start, modules_end) = {
        let mut mod_min = usize::max_value();
        let mut mod_max = 0;
        use core::cmp::{max, min};

        for m in boot_info.module_tags() {
            mod_min = min(mod_min, m.start_address() as usize);
            mod_max = max(mod_max, m.end_address() as usize);
        }
        (mod_min, mod_max)
    };
    // print_early!("Modules physical memory region: start {:#X} to end {:#X}", modules_start, modules_end);

    let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
    let mut occup_index = 0;
    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::zero(), 0x10_0000, 1, 0); // reserve addresses under 1 MB
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(kernel_phys_start, kernel_phys_end.value() - kernel_phys_start.value(), 1, 0); // the kernel boot image is already in use
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::new(boot_info.start_address() - KERNEL_OFFSET)?, boot_info.end_address() - boot_info.start_address(), 1, 0); // preserve bootloader info
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::new(modules_start)?, modules_end - modules_start, 1, 0); // preserve all modules
    occup_index += 1;


    // init the frame allocator with the available memory sections and the occupied memory sections
    let fa = AreaFrameAllocator::new(available, avail_index, occupied, occup_index)?;
    let frame_allocator_mutex: &MutexIrqSafe<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new( fa ) 
    });

    // print_early!("Boot info: {:?}\n", boot_info);


    // Initialize paging (create a new page table), which also initializes the kernel heap.

    let (
        page_table,
        kernel_vmas,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages,
    ) = paging::init(frame_allocator_mutex, &boot_info)?;

    // HERE: heap is initialized! Can now use alloc types.
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.   

    debug!("Done with paging::init()!, page_table: {:?}", page_table);

    
    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_BOTTOM)); 
        let stack_alloc_end = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_TOP_ADDR));
        let stack_alloc_range = PageRange::new(stack_alloc_start, stack_alloc_end);
        StackAllocator::new(stack_alloc_range, false)
    };

    // return the kernel's memory info 
    let kernel_mmi = MemoryManagementInfo {
        page_table: page_table,
        vmas: kernel_vmas,
        extra_mapped_pages: higher_half_mapped_pages,
        stack_allocator: kernel_stack_allocator, 
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok( (kernel_mmi_ref.clone(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) )
}