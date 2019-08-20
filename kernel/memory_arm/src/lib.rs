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
use kernel_config::memory::aarch64::{KERNEL_OFFSET, KERNEL_OFFSET_BITS_START, KERNEL_OFFSET_PREFIX, HARDWARE_START, HARDWARE_END};
use bit_field::BitField;
use uefi::prelude::*;
use uefi::table::boot::{MemoryDescriptor, MemoryType};

pub type BootInformation = BootServices;

/// Initialize the memory system
pub fn arch_init(bt: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    // get memory layout information
    const EXTRA_MEMORY_INFO_BUFFER_SIZE: usize = 8;
    let mapped_info_size =
        bt.memory_map_size() + EXTRA_MEMORY_INFO_BUFFER_SIZE * mem::size_of::<MemoryDescriptor>();

    let mut buffer = Vec::with_capacity(mapped_info_size);
    unsafe {
        buffer.set_len(mapped_info_size);
    }
    let (_key, mut maps_iter) = bt
        .memory_map(&mut buffer)
        .expect_success("Failed to retrieve UEFI memory map");

    // parse memory layout information
    // let mut kernel_phys_start: PhysicalAddress = PhysicalAddress::new(0)?;
    // let mut kernel_phys_end: PhysicalAddress = PhysicalAddress::new(0)?;
    let mut avail_index = 0;
    let mut available: [PhysicalMemoryArea; 32] = Default::default();

    let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
    let mut occup_index = 0;

    const DEFAULT: usize = 0;
    const IMAGE_START: usize = 1;
    const UEFI_START: usize = 2;
    let mut address_section = DEFAULT;

    let mut uefi_phys_start: PhysicalAddress = PhysicalAddress::new(0)?;
    let mut uefi_phys_end: PhysicalAddress = PhysicalAddress::new(0)?;
    loop {
        match maps_iter.next() {
            Some(mapped_pages) => {
                let phys_start = mapped_pages.phys_start as usize;
                let size = mapped_pages.page_count as usize * PAGE_SIZE;

                match mapped_pages.ty {
                    MemoryType::CONVENTIONAL => {
                        if address_section == DEFAULT {
                            available[avail_index] = PhysicalMemoryArea {
                                base_addr: PhysicalAddress::new(phys_start)?,
                                size_in_bytes: size,
                                typ: 1,
                                acpi: 0,
                            };
                            avail_index += 1;
                            address_section = IMAGE_START;
                        } else {
                            uefi_phys_end = PhysicalAddress::new(phys_start + size)?
                        }
                    }
                    MemoryType::LOADER_DATA => {
                        if address_section == IMAGE_START {
                            occupied[occup_index] = PhysicalMemoryArea {
                                base_addr: PhysicalAddress::new(phys_start)?,
                                size_in_bytes: size,
                                typ: 1,
                                acpi: 0,
                            };
                            occup_index += 1;
                        } else {
                            uefi_phys_end = PhysicalAddress::new(phys_start + size)?
                        }
                    }
                    MemoryType::LOADER_CODE => {
                        if address_section == IMAGE_START {
                            occupied[occup_index] = PhysicalMemoryArea {
                                base_addr: PhysicalAddress::new(phys_start)?,
                                size_in_bytes: size,
                                typ: 1,
                                acpi: 0,
                            };
                            address_section = UEFI_START
                        } else {
                            uefi_phys_end = PhysicalAddress::new(phys_start + size)?
                        }
                    }
                    MemoryType::MMIO => {
                        occupied[occup_index] = PhysicalMemoryArea {
                            base_addr: PhysicalAddress::new(phys_start)?,
                            size_in_bytes: size,
                            typ: 1,
                            acpi: 0,
                        };
                        occup_index += 1;
                    }
                    _ => {
                        if uefi_phys_start.value() == 0 {
                            uefi_phys_start = PhysicalAddress::new(phys_start as usize)?;
                        }
                        uefi_phys_end = PhysicalAddress::new(phys_start + size)?;
                    }
                }

                debug!(
                    "Memory area start:{:#X} size:{:#X} type {:?}\n",
                    phys_start, size, mapped_pages.ty
                );
            }
            None => break,
        }

        //mapped_pages_index += 1;
    }

    occupied[occup_index] = PhysicalMemoryArea::new(
        uefi_phys_start,
        uefi_phys_end.value() - uefi_phys_start.value(),
        1,
        0,
    ); // kernel
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(
        PhysicalAddress::new_canonical(HARDWARE_START as usize),
        (HARDWARE_END - HARDWARE_START) as usize,
        1,
        0,
    ); // hardware

    let uefi_virt_end = uefi_phys_end + KERNEL_OFFSET;
    debug!(
        "uefi_phys_start: {:#X}, uefi_phys_end: {:#X} uefi_virt_end = {:#X}",
        uefi_phys_start, uefi_phys_end, uefi_virt_end
    );

    // UEFI uses the Load File Protocol to load modules
    // // calculate the bounds of physical memory that is occupied by modules we've loaded
    // // (we can reclaim this later after the module is loaded, but not until then)
    // let (modules_start, modules_end) = {
    //     let mut mod_min = usize::max_value();
    //     let mut mod_max = 0;
    //     use core::cmp::{max, min};

    //     for m in boot_info.module_tags() {
    //         mod_min = min(mod_min, m.start_address() as usize);
    //         mod_max = max(mod_max, m.end_address() as usize);
    //     }
    //     (mod_min, mod_max)
    // };
    // // print_early!("Modules physical memory region: start {:#X} to end {:#X}", modules_start, modules_end);

    // init the frame allocator with the available memory sections and the occupied memory sections
    let fa = AreaFrameAllocator::new(available, avail_index, occupied, occup_index)?;
    let frame_allocator_mutex: &MutexIrqSafe<AreaFrameAllocator> =
        FRAME_ALLOCATOR.call_once(|| MutexIrqSafe::new(fa));

    // Initialize paging (create a new page table), which also initializes the kernel heap.
    let (
        page_table,
        kernel_vmas,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages,
    ) = try!(paging::init(bt, frame_allocator_mutex));
    // HERE: heap is initialized! Can now use alloc types.

    debug!("Done with paging::init()!, active_table: {:?}", page_table);

    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start =
            Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_BOTTOM));
        let stack_alloc_end = Page::containing_address(VirtualAddress::new_canonical(
            KERNEL_STACK_ALLOCATOR_TOP_ADDR,
        ));
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

    let kernel_mmi_ref = KERNEL_MMI.call_once(|| Arc::new(MutexIrqSafe::new(kernel_mmi)));

    Ok((
        kernel_mmi_ref.clone(),
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        identity_mapped_pages,
    ))
}
