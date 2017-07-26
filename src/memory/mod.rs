// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::*; //{Page, PageIter, PageTable, ActivePageTable, InactivePageTable, PhysicalAddress, VirtualAddress, EntryFlags};
pub use self::stack_allocator::{StackAllocator, Stack};

mod config;
mod area_frame_allocator;
mod paging;
mod stack_allocator;

use multiboot2::BootInformation;
use spin::{Once, Mutex};
use core::ops::DerefMut;
use alloc::arc::Arc;
use collections::Vec;
use collections::string::String;
pub use self::config::*;


pub type PhysicalAddress = usize;
pub type VirtualAddress = usize;



/// The one and only frame allocator
pub static FRAME_ALLOCATOR: Once<Mutex<AreaFrameAllocator>> = Once::new();

pub fn allocate_frame() -> Option<Frame> {
    let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock(); 
    frame_allocator.allocate_frame()
}


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
/// A kernel stack allocator is required, but a userspace one is not (kernel threads don't have them).
pub struct MemoryManagementInfo {
    /// the PageTable enum (Active or Inactive depending on whether the Task is running) 
    pub page_table: PageTable,
    
    /// the list of virtual memory areas mapped currently in this Task's address space
    pub vmas: Vec<VirtualMemoryArea>,
    
    /// there is a single kernel stack allocator instance, we just keep a cloned Arc reference to it here.
    pub kernel_stack_allocator: Arc<Mutex<stack_allocator::StackAllocator>>, // TODO: this shouldn't be public, once we move spawn_userspace code into this module

    /// the task's stack allocator, which is initialized with a range of Pages from which to allocate.
    pub user_stack_allocator: Option<stack_allocator::StackAllocator>,  // TODO: this shouldn't be public, once we move spawn_userspace code into this module
}

impl MemoryManagementInfo {

    // pub fn new(stack_allocator: stack_allocator::StackAllocator) -> Self {
    //     MemoryManagementInfo {
    //         page_table: PageTable::Uninitialized,
    //         vmas: Vec::new(),
    //         stack_allocator: stack_allocator,
    //     }
    // }

    pub fn add_vma(&mut self, vma: VirtualMemoryArea) {
        self.vmas.push(vma);
    }

    pub fn add_vmas(&mut self, vmas: &mut Vec<VirtualMemoryArea>) {
        self.vmas.append(vmas);
    }

    pub fn set_page_table(&mut self, pgtbl: PageTable) {
        self.page_table = pgtbl;
    }


    /// Allocates a new kernel stack in the currently-running Task's address space.
    /// The task that called this must be currently running! 
    /// This checks to make sure that this struct's page_table is an ActivePageTable.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    pub fn alloc_stack_kernel(&mut self, size_in_pages: usize) -> Option<Stack> {
        self.alloc_stack(size_in_pages, false)
    }


    /// Allocates a new user stack in the currently-running Task's address space.
    /// The task that called this must be currently running! 
    /// This checks to make sure that this struct's page_table is an ActivePageTable.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    pub fn alloc_stack_user(&mut self, size_in_pages: usize) -> Option<Stack> {
        self.alloc_stack(size_in_pages, true)
    }


    fn alloc_stack(&mut self, size_in_pages: usize, usermode: bool) -> Option<Stack> {
        let &mut MemoryManagementInfo { ref mut page_table,
                                        ref mut vmas,
                                        ref mut kernel_stack_allocator,
                                        ref mut user_stack_allocator } = self;                                           
    
        match page_table {
            &mut PageTable::Active(ref mut active_table) => {
                let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();

                if usermode {
                    if let Some( (ustack, stack_vma) ) = user_stack_allocator.as_mut().expect("MMI::alloc_stack: user_stack_allocator was None!")
                                                                            .alloc_stack(active_table, frame_allocator.deref_mut(), size_in_pages) {
                        vmas.push(stack_vma);
                        Some(ustack)
                    }
                    else {
                        error!("MemoryManagementInfo::alloc_stack: failed to allocate user stack!");
                        None
                    }

                } else {
                    let mut ksa = kernel_stack_allocator.lock();
                    if let Some( (kstack, stack_vma) ) = ksa.alloc_stack(active_table, frame_allocator.deref_mut(), size_in_pages) {
                        vmas.push(stack_vma);
                        Some(kstack)
                    }
                    else {
                        error!("MemoryManagementInfo::alloc_stack: failed to allocate kernel stack!");
                        None
                    }
                    
                }
            }
            _ => {
                // panic, because this should never happen
                panic!("MemoryManagementInfo::alloc_stack: page_table wasn't an ActivePageTable!");
                None
            }
        }
    }
}




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

/// An area of physical memory that contains a userspace module
/// as provided by the multiboot2-compliant bootloader
#[derive(Copy, Clone, Debug, Default)]
pub struct ModuleArea {
    mod_start: u32,
    mod_end: u32,
    name: &'static str,
}

impl ModuleArea {
    pub fn start_address(&self) -> PhysicalAddress {
        self.mod_start as PhysicalAddress
    }

    pub fn size(&self) -> usize {
        (self.mod_end - self.mod_start) as usize
    }

    pub fn name(&self) -> &'static str {
        self.name
    }
}


/// A region of virtual memory that is mapped into a `Task`'s address space
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VirtualMemoryArea {
    start: VirtualAddress,
    size: usize,
    flags: EntryFlags,
    desc: &'static str,
}


impl VirtualMemoryArea {
    pub fn new(start: VirtualAddress, size: usize, flags: EntryFlags, desc: &'static str) -> Self {
        VirtualMemoryArea {
            start: start,
            size: size,
            flags: flags,
            desc: desc,
        }
    }

    pub fn start_address(&self) -> VirtualAddress {
        self.start
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn flags(&self) -> EntryFlags {
        self.flags
    }

    pub fn desc(&self) -> &'static str {
        self.desc
    }

    /// Get an iterator that covers all the pages in this VirtualMemoryArea
    pub fn pages(&self) -> PageIter {

        // check that the end_page won't be invalid
        if (self.start + self.size) < 1 {
            // return an "empty" iterator (one that goes from 1 to 0, so no iterations happen)
            return Page::range_inclusive( Page::containing_address(PAGE_SIZE), Page::containing_address(0) );
        }
        
        let start_page = Page::containing_address(self.start);
        let end_page = Page::containing_address((self.start as usize + self.size - 1) as VirtualAddress);
        Page::range_inclusive(start_page, end_page)
    }

    // /// Convert this memory zone to a shared one.
    // pub fn to_shared(self) -> SharedMemory {
    //     SharedMemory::Owned(Arc::new(Mutex::new(self)))
    // }

    // /// Map a new space on the virtual memory for this memory zone.
    // fn map(&mut self, clean: bool) {
    //     // create a new active page table
    //     let mut active_table = unsafe { ActivePageTable::new() };

    //     // get memory controller
    //     if let Some(ref mut memory_controller) = *::MEMORY_CONTROLLER.lock() {
    //         for page in self.pages() {
    //             memory_controller.map(&mut active_table, page, self.flags);
    //         }
    //     } else {
    //         panic!("Memory controller required");
    //     }
    // }

    // /// Remap a memory area to another region
    // pub fn remap(&mut self, new_flags: EntryFlags) {
    //     // create a new page table
    //     let mut active_table = unsafe { ActivePageTable::new() };

    //     // get memory controller
    //     if let Some(ref mut memory_controller) = *::MEMORY_CONTROLLER.lock() {
    //         // remap all pages
    //         for page in self.pages() {
    //             memory_controller.remap(&mut active_table, page, new_flags);
    //         }

    //         // flush TLB
    //         memory_controller.flush_all();

    //         self.flags = new_flags;
    //     } else {
    //         panic!("Memory controller required");
    //     }
    // }

}



/// The set of physical memory areas as provided by the bootloader.
/// It cannot be a Vec or other collection because those allocators aren't available yet
/// we use a max size of 32 because that's the limit of Rust's default array initializers
static USABLE_PHYSICAL_MEMORY_AREAS: Once<[PhysicalMemoryArea; MAX_MEMORY_AREAS]> = Once::new();

/// The set of modules loaded by the bootloader
/// we use a max size of 32 because that's the limit of Rust's default array initializers
static MODULE_AREAS: Once<[ModuleArea; MAX_MEMORY_AREAS]> = Once::new();


/// initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// The returned MemoryManagementInfo struct is partially initialized with the kernel's StackAllocator instance, 
/// and the list of `VirtualMemoryArea`s that represent some of the kernel's mapped sections (for task zero).
pub fn init(boot_info: &BootInformation) -> MemoryManagementInfo {
    assert_has_not_been_called!("memory::init must be called only once");

    // copy the list of modules (currently used for userspace programs)
    MODULE_AREAS.call_once( || {
        let mut modules: [ModuleArea; MAX_MEMORY_AREAS] = Default::default();
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
        let mut areas: [PhysicalMemoryArea; MAX_MEMORY_AREAS] = Default::default();
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

    let mut kernel_vmas: [VirtualMemoryArea; MAX_MEMORY_AREAS] = Default::default();
    let mut active_table = paging::remap_the_kernel(frame_allocator_mutex.lock().deref_mut(), boot_info, &mut kernel_vmas);


    // The heap memory must be mapped before it can initialized! Map it and then init it here. 
    use self::paging::Page;
    use heap_irq_safe;
    let heap_start_page = Page::containing_address(KERNEL_HEAP_START);
    let heap_end_page = Page::containing_address(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE - 1);
    let heap_flags = paging::WRITABLE;
    let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
    for page in Page::range_inclusive(heap_start_page, heap_end_page) {
        active_table.map(page, heap_flags, frame_allocator_mutex.lock().deref_mut());
    }
    heap_irq_safe::init(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);


    // HERE: now the heap is set up, we can use dynamically-allocated collections types like Vecs


    let mut task_zero_vmas: Vec<VirtualMemoryArea> = kernel_vmas.to_vec();
    task_zero_vmas.retain(|x|  *x != VirtualMemoryArea::default() );
    task_zero_vmas.push(heap_vma);

    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start = Page::containing_address(KERNEL_STACK_ALLOCATOR_BOTTOM); 
        let stack_alloc_end = Page::containing_address(KERNEL_STACK_ALLOCATOR_TOP_ADDR);
        let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
        stack_allocator::StackAllocator::new(stack_alloc_range, false)
    };


    // return the kernel's (task_zero's) memory info 
    MemoryManagementInfo {
        page_table: PageTable::Active(active_table),
        vmas: task_zero_vmas,
        kernel_stack_allocator: Arc::new(Mutex::new(kernel_stack_allocator)), 
        user_stack_allocator: None,
    }

}


/// returns the `ModuleArea` corresponding to the given `index`
pub fn get_module(index: usize) -> Option<&'static ModuleArea> {
    let modules = MODULE_AREAS.try().expect("get_module(): MODULE_AREAS not yet initialized.");
    if index < modules.len() {
        Some(&modules[index])
    }
    else {
        None
    }
}



#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    number: usize,
}

impl Frame {
	/// returns the Frame containing the given physical address
    pub fn containing_address(address: usize) -> Frame {
        Frame { number: address / PAGE_SIZE }
    }

    fn start_address(&self) -> PhysicalAddress {
        self.number * PAGE_SIZE
    }

    pub fn clone(&self) -> Frame {
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
