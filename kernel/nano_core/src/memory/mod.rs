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

mod area_frame_allocator;
mod paging;
mod stack_allocator;
pub mod virtual_address_allocator;


use multiboot2::BootInformation;
use spin::{Once, Mutex};
use core::ops::DerefMut;
use alloc::Vec;
use alloc::string::String;
use alloc::arc::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_OFFSET, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR};
use task;
use mod_mgmt::{parse_elf_kernel_crate, parse_nano_core};
use mod_mgmt::metadata;
use irq_safety::MutexIrqSafe;

pub type PhysicalAddress = usize;
pub type VirtualAddress = usize;



/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<Arc<MutexIrqSafe<MemoryManagementInfo>>> = Once::new();

/// returns the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns None.
pub fn get_kernel_mmi_ref() -> Option<Arc<MutexIrqSafe<MemoryManagementInfo>>> {
    KERNEL_MMI.try().cloned()
}


/// The one and only frame allocator, a singleton. 
pub static FRAME_ALLOCATOR: Once<Mutex<AreaFrameAllocator>> = Once::new();

/// Convenience method for allocating a new Frame.
pub fn allocate_frame() -> Option<Frame> {
    let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock(); 
    frame_allocator.allocate_frame()
}


/// A copy of the set of modules loaded by the bootloader
static MODULE_AREAS: Once<Vec<ModuleArea>> = Once::new();


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
pub struct MemoryManagementInfo {
    /// the PageTable enum (Active or Inactive depending on whether the Task is running) 
    pub page_table: PageTable,
    
    /// the list of virtual memory areas mapped currently in this Task's address space
    pub vmas: Vec<VirtualMemoryArea>,

    /// the task's stack allocator, which is initialized with a range of Pages from which to allocate.
    pub stack_allocator: stack_allocator::StackAllocator,  // TODO: this shouldn't be public, once we move spawn_userspace code into this module
}

impl MemoryManagementInfo {

    // pub fn new(stack_allocator: stack_allocator::StackAllocator) -> Self {
    //     MemoryManagementInfo {
    //         page_table: PageTable::Uninitialized,
    //         vmas: Vec::new(),
    //         stack_allocator: stack_allocator,
    //     }
    // }

    pub fn set_page_table(&mut self, pgtbl: PageTable) {
        self.page_table = pgtbl;
    }


    /// Allocates a new stack in the currently-running Task's address space.
    /// The task that called this must be currently running! 
    /// This checks to make sure that this struct's page_table is an ActivePageTable.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    /// Whether this is a kernelspace or userspace stack is determined by how this MMI's stack_allocator was initialized.
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryManagementInfo { ref mut page_table, ref mut vmas, ref mut stack_allocator } = self;
    
        match page_table {
            &mut PageTable::Active(ref mut active_table) => {
                let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();

                if let Some( (stack, stack_vma) ) = stack_allocator.alloc_stack(active_table, frame_allocator.deref_mut(), size_in_pages) {
                    vmas.push(stack_vma);
                    Some(stack)
                }
                else {
                    error!("MemoryManagementInfo::alloc_stack: failed to allocate stack!");
                    None
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
    pub base_addr: usize,
    pub length: usize,
    pub typ: u32,
    pub acpi: u32
}
impl PhysicalMemoryArea {
    pub fn new(addr: usize, len: usize, typ: u32, acpi: u32) -> PhysicalMemoryArea {
        PhysicalMemoryArea {
            base_addr: addr,
            length: len,
            typ: typ,
            acpi: acpi,
        }
    }
}

// #[derive(Clone)]
// pub struct PhysicalMemoryAreaIter {
//     index: usize
// }

// impl PhysicalMemoryAreaIter {
//     pub fn new() -> Self {
//         PhysicalMemoryAreaIter {
//             index: 0
//         }
//     }
// }

// impl Iterator for PhysicalMemoryAreaIter {
//     type Item = &'static PhysicalMemoryArea;
//     fn next(&mut self) -> Option<&'static PhysicalMemoryArea> {
//         let areas = USABLE_PHYSICAL_MEMORY_AREAS.try().expect("USABLE_PHYSICAL_MEMORY_AREAS was used before initialization!");
//         while self.index < areas.len() {
//             // get the entry in the current index
//             let entry = &areas[self.index];

//             // increment the index
//             self.index += 1;

//             if entry.typ == 1 {
//                 return Some(entry)
//             }
//         }

//         None
//     }
// }

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
use core::fmt;
impl fmt::Display for VirtualMemoryArea {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "start: {:#X}, size: {:#X}, flags: {:#X}, desc: {}", 
                  self.start, self.size, self.flags, self.desc
        )
    }
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






/// initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessibl.e
/// The returned MemoryManagementInfo struct is partially initialized with the kernel's StackAllocator instance, 
/// and the list of `VirtualMemoryArea`s that represent some of the kernel's mapped sections (for task zero).
pub fn init(boot_info: BootInformation) -> Result<Arc<MutexIrqSafe<MemoryManagementInfo>>, &'static str> {
    assert_has_not_been_called!("memory::init must be called only once");
    debug!("memory::init() at top!");
    let rsdt_phys_addr = boot_info.acpi_old_tag().and_then(|acpi| acpi.get_rsdp().map(|rsdp| rsdp.rsdt_phys_addr()));
    debug!("rsdt_phys_addr: {:#X}", if let Some(pa) = rsdt_phys_addr { pa } else { 0 });
    
    let memory_map_tag = try!(boot_info.memory_map_tag().ok_or("Memory map tag not found"));
    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("Elf sections tag not found"));

    // Our linker script specifies that the kernel will have the .init section starting at 1MB and ending at 1MB + .init size
    // and all other kernel sections will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // Thus, kernel_phys_start is the same as kernel_virt_start initially, but we remap them later in remap_the_kernel.
    let kernel_phys_start: PhysicalAddress = try!(elf_sections_tag.sections()
        .filter(|s| s.is_allocated())
        .map(|s| s.start_address())
        .min()
        .ok_or("Couldn't find kernel start address")) as PhysicalAddress;
    let kernel_virt_end: VirtualAddress = try!(elf_sections_tag.sections()
        .filter(|s| s.is_allocated())
        .map(|s| s.end_address())
        .max()
        .ok_or("Couldn't find kernel end address")) as PhysicalAddress;
    let kernel_phys_end: PhysicalAddress = kernel_virt_end - KERNEL_OFFSET;


    debug!("kernel_phys_start: {:#x}, kernel_phys_end: {:#x} kernel_virt_end = {:#x}",
             kernel_phys_start,
             kernel_phys_end,
             kernel_virt_end);
    debug!("multiboot start: {:#x}, multiboot end: {:#x}",
             boot_info.start_address(),
             boot_info.end_address());
    
    
    // parse the list of physical memory areas from multiboot
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
    for area in memory_map_tag.memory_areas() {
        debug!("memory area base_addr={:#x} length={:#x} ({:?})", area.start_address(), area.size(), area);
        
        // optimization: we reserve memory from areas below the end of the kernel's physical address,
        // which includes addresses beneath 1 MB
        if area.end_address() < kernel_phys_end {
            debug!("  skipping region before kernel_phys_end");
            continue;
        }
        let start_paddr: PhysicalAddress = if area.start_address() >= kernel_phys_end { area.start_address() } else { kernel_phys_end };

        available[avail_index] = PhysicalMemoryArea {
            base_addr: start_paddr,
            length: area.end_address() - start_paddr,
            typ: 1, 
            acpi: 0, 
        };

        info!("  region established: start={:#x}, length={:#x}", available[avail_index].base_addr, available[avail_index].length);
        avail_index += 1;
    }


    // init the frame allocator
    let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
    occupied[0] = PhysicalMemoryArea::new(0, 0x10_0000, 1, 0); // reserve addresses under 1 MB
    occupied[1] = PhysicalMemoryArea::new(kernel_phys_start, kernel_phys_end-kernel_phys_start, 1, 0); // the kernel boot image is already in use
    occupied[2] = PhysicalMemoryArea::new(boot_info.start_address() - KERNEL_OFFSET, boot_info.end_address()-boot_info.start_address(), 1, 0); // preserve bootloader info (optional)

    let fa = try!( AreaFrameAllocator::new(available, avail_index, occupied, 3));
    let frame_allocator_mutex: &Mutex<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        Mutex::new( fa ) 
    });

    let mut kernel_vmas: [VirtualMemoryArea; 32] = Default::default();
    let mut active_table = paging::remap_the_kernel(frame_allocator_mutex.lock().deref_mut(), &boot_info, &mut kernel_vmas).unwrap();


    // The heap memory must be mapped before it can initialized! Map it and then init it here. 
    use self::paging::Page;
    use heap_irq_safe;
    let heap_start_page = Page::containing_address(KERNEL_HEAP_START);
    let heap_end_page = Page::containing_address(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE - 1);
    let heap_flags = paging::EntryFlags::WRITABLE;
    let heap_vma: VirtualMemoryArea = VirtualMemoryArea::new(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, heap_flags, "Kernel Heap");
    for page in Page::range_inclusive(heap_start_page, heap_end_page) {
        active_table.map(page, heap_flags, frame_allocator_mutex.lock().deref_mut());
    }
    heap_irq_safe::init(KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE);

    // HERE: now the heap is set up, we can use dynamically-allocated types like Vecs
    {
        frame_allocator_mutex.lock().alloc_ready();
    }


    // copy the list of modules (currently used for userspace programs)
    MODULE_AREAS.call_once( || {
        let mut modules: Vec<ModuleArea> = Vec::new();
        for m in boot_info.module_tags() {
            // debug!("Module: {:?}", m);
            let mod_area = ModuleArea {
                mod_start: m.start_address(), 
                mod_end:   m.end_address(), 
                name:      m.name(),
            };
            debug!("Module: {:?}", mod_area);
            modules.push(mod_area);
        }
        modules
    });


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
    let kernel_mmi = MemoryManagementInfo {
        page_table: PageTable::Active(active_table),
        vmas: task_zero_vmas,
        stack_allocator: kernel_stack_allocator, 
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok(kernel_mmi_ref.clone())

}


/// Loads the specified kernel crate into memory, allowing it to be invoked.  
/// Returns a Result containing the number of symbols that were added to the system map
/// as a result of loading this crate.
pub fn load_kernel_crate(module: &ModuleArea, kernel_mmi: &mut MemoryManagementInfo) -> Result<usize, &'static str> {
    debug!("load_kernel_crate: trying to load \"{}\" kernel module", module.name());
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(module.start_address()) {
        error!("module {} is not page aligned!", module.name());
        return Err("module was not page aligned");
    } 

    // first we need to map the module memory region into our address space, 
    // so we can then parse the module as an ELF file in the kernel.
    // For now just use identity mapping, we can use identity mapping here because we have a higher-half mapped kernel, YAY! :)
    {
        // destructure the kernel's MMI so we can access its page table and vmas
        let &mut MemoryManagementInfo { 
            page_table: ref mut kernel_page_table, 
            ..  // don't need to access the kernel's vmas or stack allocator, we already allocated a kstack above
        } = kernel_mmi;
            

        // // temporarily dumping kernel VMAs
        // {
        //     info!("================ KERNEL VMAS ================");
        //     for vma in kernel_vmas {
        //         info!("   {}", vma);
        //     }
        // }

        match kernel_page_table {
            &mut PageTable::Active(ref mut active_table) => {
                let module_flags = EntryFlags::PRESENT;
                {
                    let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();
                    active_table.map_frames(Frame::range_inclusive_addr(module.start_address(), module.size()), 
                                            Page::containing_address(module.start_address() as VirtualAddress), // identity mapping
                                            module_flags, frame_allocator.deref_mut());  
                }

                let new_crate = try!( {
                    // the nano_core requires special handling because it has already been loaded,
                    // we just need to parse its symbols and add them to the symbol table & crate metadata lists
                    if module.name() == "__k_nano_core" {
                        parse_nano_core(module.start_address(), module.size())
                    }
                    else {
                        parse_elf_kernel_crate(module.start_address(), module.size(), module.name(), active_table)
                    }
                });

                // now we can unmap the module because we're done reading from it in the ELF parser
                {
                    let mut frame_allocator = FRAME_ALLOCATOR.try().unwrap().lock();
                    active_table.unmap_pages(Page::range_inclusive_addr(module.start_address(), module.size()), frame_allocator.deref_mut());
                }

                info!("loaded new crate: {}", new_crate.crate_name);
                Ok(metadata::add_crate(new_crate))

            }
            _ => {
                error!("load_kernel_crate(): error getting kernel's active page table to map module.");
                Err("couldn't get kernel's active page table")
            }
        }
    }

}


/// returns the `ModuleArea` corresponding to the given `index`
pub fn get_module_index(index: usize) -> Result<&'static ModuleArea, &'static str> {
    let modules = try!(MODULE_AREAS.try().ok_or("MODULE_AREAS not initialized"));
    modules.get(index).ok_or("module index out of range")
}


/// returns the `ModuleArea` corresponding to the given module name.
pub fn get_module(name: &str) -> Result<&'static ModuleArea, &'static str> {
    let modules = try!(MODULE_AREAS.try().ok_or("MODULE_AREAS not initialized"));
    modules.iter().filter(|&&m| m.name == name).next().ok_or("module not found")
}


/// returns the `ModuleArea` corresponding to the given module name.
pub fn get_module(name: &str) -> Option<&'static ModuleArea> {
    let ma_pair = MODULE_AREAS.try().expect("get_module(): MODULE_AREAS not yet initialized.");
    for i in 0..ma_pair.1 {
        if name == ma_pair.0[i].name() {
            return Some(&ma_pair.0[i]);
        }
    }

    // not found    
    None
}



#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Frame {
    number: usize,
}
impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Frame(paddr: {:#X})", self.start_address()) 
    }
}

impl Frame {
	/// returns the Frame containing the given physical address
    pub fn containing_address(phys_addr: usize) -> Frame {
        Frame { number: phys_addr / PAGE_SIZE }
    }

    pub fn start_address(&self) -> PhysicalAddress {
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

    pub fn range_inclusive_addr(phys_addr: PhysicalAddress, size_in_bytes: usize) -> FrameIter {
        FrameIter {
            start: Frame::containing_address(phys_addr),
            end: Frame::containing_address(phys_addr + size_in_bytes - 1),
        }
    }
}

use core::ops::{Add, AddAssign, Sub, SubAssign};
impl Add<usize> for Frame {
    type Output = Frame;

    fn add(self, rhs: usize) -> Frame {
        assert!(self.number < MAX_PAGE_NUMBER, "Frame addition error, cannot go above MAX_PAGE_NUMBER 0x000FFFFFFFFFFFFF!");
        Frame { number: self.number + rhs }
    }
}

impl AddAssign<usize> for Frame {
    fn add_assign(&mut self, rhs: usize) {
        *self = Frame {
            number: self.number + rhs,
        };
    }
}

impl Sub<usize> for Frame {
    type Output = Frame;

    fn sub(self, rhs: usize) -> Frame {
        assert!(self.number > 0, "Frame subtraction error, cannot go below zero!");
        Frame { number: self.number - rhs }
    }
}

impl SubAssign<usize> for Frame {
    fn sub_assign(&mut self, rhs: usize) {
        *self = Frame {
            number: self.number - rhs,
        };
    }
}

pub struct FrameIter {
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
