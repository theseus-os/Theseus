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

mod area_frame_allocator;
mod paging;
mod stack_allocator;

pub const PAGE_SIZE: usize = 4096;

/// the virtual address where the kernel is mapped to.
/// i.e., the linear offset between physical memory and kernel memory
/// so the VGA buffer will be moved from 0xb8000 to 0xFFFFFFFF800b8000.
pub const KERNEL_OFFSET: usize = 0xFFFFFFFF80000000;


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
        while self.index < unsafe { USABLE_PHYSICAL_MEMORY_AREAS.len() } {
            // get the entry in the current index
            let entry = unsafe { &USABLE_PHYSICAL_MEMORY_AREAS[self.index] };

            // increment the index
            self.index += 1;

            if entry.typ == 1 {
                return Some(entry)
            }
        }

        None
    }
}

/// The set of physical memory areas as provided by the bootloader.
/// It cannot be a Vec or other collection because those allocators aren't available yet
static mut USABLE_PHYSICAL_MEMORY_AREAS: [PhysicalMemoryArea; 512] = [PhysicalMemoryArea { base_addr: 0, length: 0, typ: 0, acpi: 0 }; 512];


pub fn init(boot_info: &BootInformation) -> MemoryController {
    assert_has_not_been_called!("memory::init must be called only once");

    let memory_map_tag = boot_info.memory_map_tag().expect("Memory map tag required");
    let elf_sections_tag = boot_info.elf_sections_tag().expect("Elf sections tag required");


    // testing module tags
    use multiboot2::ModuleIter;
    let mut modules_iter: ModuleIter = boot_info.module_tags();
    let mut tags_iter = boot_info.tags();
    for tag in tags_iter {
        println_unsafe!("Multiboot2 TAG: typ:{} size:{:#x}", tag.typ, tag.size);
    }
    for (i, module) in modules_iter.enumerate() {
        println_unsafe!("Module {}: {:?}", i, module);
    }

    

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
    let mut index = 0;
    for area in memory_map_tag.memory_areas() {
        println_unsafe!("memory area base_addr={:#x} length={:#x}", area.base_addr, area.length);
        
        // we cannot allocate memory from sections beneath the end of the kernel's physical address!!
        if area.base_addr + area.length < kernel_phys_end {
            println_unsafe!("  skipping region before kernel_phys_end");
            continue;
        }

        unsafe {
            let mut entry = &mut USABLE_PHYSICAL_MEMORY_AREAS[index];
            entry.base_addr = if area.base_addr >= kernel_phys_end { area.base_addr } else { kernel_phys_end };
            entry.length = (area.base_addr + area.length) - entry.base_addr;
            entry.typ = 1;

            println_unsafe!("  region established: start={:#x}, length={:#x}", entry.base_addr, entry.length);
        }
        index += 1;
    }



    let mut frame_allocator = AreaFrameAllocator::new(kernel_phys_start as usize,
                                                      kernel_phys_end as usize,
                                                      boot_info.start_address(),
                                                      boot_info.end_address(),
                                                      PhysicalMemoryAreaIter::new());

    let mut active_table = paging::remap_the_kernel(&mut frame_allocator, boot_info);

    use self::paging::Page;
    use hole_list_allocator::{HEAP_START, HEAP_SIZE};

    let heap_start_page = Page::containing_address(HEAP_START);
    let heap_end_page = Page::containing_address(HEAP_START + HEAP_SIZE - 1);

    // map the entire heap to randomly chosen physical Frames
    let mut ctr = 0;
    for page in Page::range_inclusive(heap_start_page, heap_end_page) {
        // println_unsafe!("mapping heap page {} at virt addr: {:#x}", ctr, page.start_address());
        ctr += 1;
        active_table.map(page, paging::WRITABLE, &mut frame_allocator);
    }

    let stack_allocator = {
        let stack_alloc_start = heap_end_page + 1; // extra stack pages start right after the heap ends
        let stack_alloc_end = stack_alloc_start + 100; // 100 pages in size
        let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
        stack_allocator::StackAllocator::new(stack_alloc_range)
    };

    MemoryController {
        active_table: active_table,
        frame_allocator: frame_allocator,
        stack_allocator: stack_allocator,
    }
}

pub struct MemoryController {
    active_table: paging::ActivePageTable,
    frame_allocator: AreaFrameAllocator,
    stack_allocator: stack_allocator::StackAllocator,
}

impl MemoryController {
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryController { ref mut active_table,
                                    ref mut frame_allocator,
                                    ref mut stack_allocator } = self;
        stack_allocator.alloc_stack(active_table, frame_allocator, size_in_pages)
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
