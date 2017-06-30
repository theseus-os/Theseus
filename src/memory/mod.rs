// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::ActivePageTable;
pub use self::paging::remap_the_kernel;
pub use self::stack_allocator::Stack;

use self::paging::PhysicalAddress;
use multiboot2::BootInformation;
use spin::{Once, Mutex};
use core::ops::DerefMut;

mod area_frame_allocator;
mod paging;
mod stack_allocator;

pub const PAGE_SIZE: usize = 4096;

/// the virtual address where the kernel is mapped to.
/// i.e., the linear offset between physical memory and kernel memory
/// so the VGA buffer will be moved from 0xb8000 to 0xFFFFFFFF800b8000.
pub const KERNEL_OFFSET: usize = 0xFFFFFFFF80000000;


const MAX_MODULES: usize = 32;
const MAX_PHYSICAL_MEM_AREAS: usize = 32;


/// The one and only frame allocator
static FRAME_ALLOCATOR: Once<Mutex<AreaFrameAllocator>> = Once::new();



/// An area of physical memory. 
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct PhysicalMemoryArea {
    pub base_addr: u64,
    pub length: u64,
    pub typ: u32,
    pub acpi: u32
}

#[derive(Clone)]
pub struct PhysicalMemoryAreaIter {
    index: usize
}

impl PhysicalMemoryAreaIter {
    pub fn new() -> Self {
        PhysicalMemoryAreaIter {
            index: 0
        }
    }
}

impl Iterator for PhysicalMemoryAreaIter {
    type Item = &'static PhysicalMemoryArea;
    fn next(&mut self) -> Option<&'static PhysicalMemoryArea> {
        let areas = USABLE_PHYSICAL_MEMORY_AREAS.try().expect("USABLE_PHYSICAL_MEMORY_AREAS was used before initialization!");
        while self.index < areas.len() {
            // get the entry in the current index
            let entry = &areas[self.index];

            // increment the index
            self.index += 1;

            if entry.typ == 1 {
                return Some(entry)
            }
        }

        None
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct ModuleArea {
    pub mod_start: u32,
    pub mod_end: u32,
    pub name: &'static str,
}







/// The set of physical memory areas as provided by the bootloader.
/// It cannot be a Vec or other collection because those allocators aren't available yet
/// we use a max size of 32 because that's the limit of Rust's default array initializers
static USABLE_PHYSICAL_MEMORY_AREAS: Once<[PhysicalMemoryArea; MAX_PHYSICAL_MEM_AREAS]> = Once::new();

/// The set of modules loaded by the bootloader
/// we use a max size of 32 because that's the limit of Rust's default array initializers
static MODULE_AREAS: Once<[ModuleArea; MAX_MODULES]> = Once::new();


pub fn init(boot_info: &BootInformation) -> MemoryController {
    assert_has_not_been_called!("memory::init must be called only once");

    // copy the list of modules (currently used for userspace programs)
    MODULE_AREAS.call_once( || {
        let mut modules: [ModuleArea; MAX_MODULES] = Default::default();
        for (i, m) in boot_info.module_tags().enumerate() {
            println_unsafe!("Module: {:?}", m);
            modules[i] = ModuleArea {
                mod_start: m.start_address(), 
                mod_end:   m.end_address(), 
                name:      m.name(),
            };
        }
        modules
    });


    let memory_map_tag = boot_info.memory_map_tag().expect("Memory map tag required");
    let elf_sections_tag = boot_info.elf_sections_tag().expect("Elf sections tag required");

    // our linker script specifies that the kernel will start at 1MB, and end at 1MB + length + KERNEL_OFFSET
    // so the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // thus, kernel_phys_start is the same as kernel_virt_start
    let kernel_phys_start = elf_sections_tag.sections()
        .filter(|s| s.is_allocated())
        .map(|s| s.addr)
        .min()
        .unwrap();
    let kernel_virt_end = elf_sections_tag.sections()
        .filter(|s| s.is_allocated())
        .map(|s| s.addr + s.size)
        .max()
        .unwrap();
    let kernel_phys_end = kernel_virt_end - (KERNEL_OFFSET as u64);


    println_unsafe!("kernel_phys_start: {:#x}, kernel_phys_end: {:#x} kernel_virt_end = {:#x}",
             kernel_phys_start,
             kernel_phys_end,
             kernel_virt_end);
    println_unsafe!("multiboot start: {:#x}, multiboot end: {:#x}",
             boot_info.start_address(),
             boot_info.end_address());
    
    
    // copy the list of physical memory areas from multiboot
    USABLE_PHYSICAL_MEMORY_AREAS.call_once( || {
        let mut areas: [PhysicalMemoryArea; MAX_PHYSICAL_MEM_AREAS] = Default::default();
        for (index, area) in memory_map_tag.memory_areas().enumerate() {
            println_unsafe!("memory area base_addr={:#x} length={:#x}", area.base_addr, area.length);
            
            // we cannot allocate memory from sections below the end of the kernel's physical address!!
            if area.base_addr + area.length < kernel_phys_end {
                println_unsafe!("  skipping region before kernel_phys_end");
                continue;
            }

            let start_addr = if area.base_addr >= kernel_phys_end { area.base_addr } else { kernel_phys_end };
            areas[index] = PhysicalMemoryArea {
                base_addr: start_addr,
                length: (area.base_addr + area.length) - start_addr,
                typ: 1, // TODO: what does this mean??
                acpi: 0, // TODO: what does this mean??
            };

            println_unsafe!("  region established: start={:#x}, length={:#x}", areas[index].base_addr, areas[index].length);
        }
        areas
    });


    // init the frame allocator
    let frame_allocator_mutex: &Mutex<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        Mutex::new( AreaFrameAllocator::new(kernel_phys_start as usize,
                                kernel_phys_end as usize,
                                boot_info.start_address(),
                                boot_info.end_address(),
                                PhysicalMemoryAreaIter::new()
                    )
        )
    });

    let mut active_table = paging::remap_the_kernel(frame_allocator_mutex.lock().deref_mut(), boot_info);

    use self::paging::Page;
    use hole_list_allocator::{HEAP_START, HEAP_SIZE};

    let heap_start_page = Page::containing_address(HEAP_START);
    let heap_end_page = Page::containing_address(HEAP_START + HEAP_SIZE - 1);

    // map the entire heap to randomly chosen physical Frames
    for page in Page::range_inclusive(heap_start_page, heap_end_page) {
        active_table.map(page, paging::WRITABLE, frame_allocator_mutex.lock().deref_mut());
    }

    let stack_allocator = {
        let stack_alloc_start = heap_end_page + 1; // extra stack pages start right after the heap ends
        let stack_alloc_end = stack_alloc_start + 100; // 100 pages in size
        let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
        stack_allocator::StackAllocator::new(stack_alloc_range)
    };

    MemoryController {
        active_table: active_table,
        frame_allocator_mutex: FRAME_ALLOCATOR.try().expect("FRAME_ALLOCATOR wasn't yet initialized when passing to MemoryController"),
        stack_allocator: stack_allocator,
    }
}


/// returns the `ModuleArea` corresponding 
pub fn get_module(index: usize) -> Option<&'static ModuleArea> {
    let modules = MODULE_AREAS.try().expect("get_module(): MODULE_AREAS not yet initialized.");
    if index < modules.len() {
        Some(&modules[index])
    }
    else {
        None
    }
}


pub struct MemoryController {
    active_table: paging::ActivePageTable,
    frame_allocator_mutex: &'static Mutex<AreaFrameAllocator>,
    stack_allocator: stack_allocator::StackAllocator,
}

impl MemoryController {
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryController { ref mut active_table,
                                    ref mut frame_allocator_mutex,
                                    ref mut stack_allocator } = self;
        stack_allocator.alloc_stack(active_table, frame_allocator_mutex.lock().deref_mut(), size_in_pages)
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    number: usize,
}

impl Frame {
	/// returns the Frame containing the given physical address
    fn containing_address(address: usize) -> Frame {
        Frame { number: address / PAGE_SIZE }
    }

    fn start_address(&self) -> PhysicalAddress {
        self.number * PAGE_SIZE
    }

    fn clone(&self) -> Frame {
        Frame { number: self.number }
    }

    fn range_inclusive(start: Frame, end: Frame) -> FrameIter {
        FrameIter {
            start: start,
            end: end,
        }
    }
}

struct FrameIter {
    start: Frame,
    end: Frame,
}

impl Iterator for FrameIter {
    type Item = Frame;

    fn next(&mut self) -> Option<Frame> {
        if self.start <= self.end {
            let frame = self.start.clone();
            self.start.number += 1;
            Some(frame)
        } else {
            None
        }
    }
}

pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn deallocate_frame(&mut self, frame: Frame);
}
