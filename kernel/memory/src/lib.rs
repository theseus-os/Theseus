//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
#![feature(unboxed_closures)]

extern crate spin;
extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate xmas_elf;
extern crate x86_64;
#[macro_use] extern crate bitflags;
extern crate heap_irq_safe;
#[macro_use] extern crate once; // for assert_has_not_been_called!()

mod area_frame_allocator;
mod paging;
mod stack_allocator;


pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::*;
pub use self::stack_allocator::{StackAllocator, Stack};


use multiboot2::BootInformation;
use spin::Once;
use irq_safety::MutexIrqSafe;
use core::ops::DerefMut;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_OFFSET, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR};


pub type PhysicalAddress = usize;
pub type VirtualAddress = usize;


/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<Arc<MutexIrqSafe<MemoryManagementInfo>>> = Once::new();

/// returns a cloned reference to the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns None.
pub fn get_kernel_mmi_ref() -> Option<Arc<MutexIrqSafe<MemoryManagementInfo>>> {
    KERNEL_MMI.try().cloned()
}


/// The one and only frame allocator, a singleton. 
pub static FRAME_ALLOCATOR: Once<MutexIrqSafe<AreaFrameAllocator>> = Once::new();

/// Convenience method for allocating a new Frame.
pub fn allocate_frame() -> Option<Frame> {
    FRAME_ALLOCATOR.try().and_then(|fa| fa.lock().allocate_frame())
}


/// Convenience method for allocating several contiguous Frames.
pub fn allocate_frames(num_frames: usize) -> Option<FrameIter> {
    FRAME_ALLOCATOR.try().and_then(|fa| fa.lock().allocate_frames(num_frames))
}

/// An Arc reference to a `MemoryManagementInfo` struct.
pub type MmiRef = Arc<MutexIrqSafe<MemoryManagementInfo>>;


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
pub struct MemoryManagementInfo {
    /// the PageTable enum (Active or Inactive depending on whether the Task is running) 
    pub page_table: PageTable,
    
    /// the list of virtual memory areas mapped currently in this Task's address space
    pub vmas: Vec<VirtualMemoryArea>,

    /// a list of additional virtual-mapped Pages that have the same lifetime as this MMI
    /// and are thus owned by this MMI, but is not all-inclusive (e.g., Stacks are excluded).
    /// Someday this could likely replace the vmas list, but VMAs offer sub-page granularity for now.
    pub extra_mapped_pages: Vec<MappedPages>,

    /// the task's stack allocator, which is initialized with a range of Pages from which to allocate.
    pub stack_allocator: stack_allocator::StackAllocator,  // TODO: this shouldn't be public, once we move spawn_userspace code into this module
}

impl MemoryManagementInfo {

    pub fn set_page_table(&mut self, pgtbl: PageTable) {
        self.page_table = pgtbl;
    }


    /// Allocates a new stack in the currently-running Task's address space.
    /// The task that called this must be currently running! 
    /// This checks to make sure that this struct's page_table is an ActivePageTable.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    /// Whether this is a kernelspace or userspace stack is determined by how this MMI's stack_allocator was initialized.
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryManagementInfo { ref mut page_table, ref mut vmas, ref mut stack_allocator, .. } = self;
    
        if let PageTable::Active(ref mut active_table) = page_table {
            if let Some( (stack, stack_vma) ) = FRAME_ALLOCATOR.try().and_then(|fa| stack_allocator.alloc_stack(active_table, fa.lock().deref_mut(), size_in_pages)) {
                vmas.push(stack_vma);
                Some(stack)
            }
            else {
                error!("MemoryManagementInfo::alloc_stack: failed to allocate stack of {} pages!", size_in_pages);
                None
            }
        }
        else {
            error!("MemoryManagementInfo::alloc_stack: page_table wasn't an ActivePageTable! \
                    You must not call alloc_stack on an MMI page table that isn't currently the ActivePageTable.");
            None
        }
    }
}



/// A convenience function that creates a new memory mapping by allocating frames that are contiguous in physical memory.
/// Returns a tuple containing the new `MappedPages` and the starting PhysicalAddress of the first frame,
/// which is a convenient way to get the physical address without walking the page tables.
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the `FRAME_ALLOCATOR` and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn create_contiguous_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<(MappedPages, PhysicalAddress), &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("e1000::create_contiguous_mapping(): couldn't allocate pages!")?;

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
        let mut frame_allocator = FRAME_ALLOCATOR.try()
            .ok_or("create_contiguous_mapping(): couldnt get FRAME_ALLOCATOR")?
            .lock();
        let frames = frame_allocator.allocate_frames(allocated_pages.size_in_pages())
            .ok_or("create_contiguous_mapping(): couldnt allocate a new frame")?;
        let starting_phys_addr = frames.start_address();
        let mp = active_table.map_allocated_pages_to(allocated_pages, frames, flags, frame_allocator.deref_mut())?;
        Ok((mp, starting_phys_addr))
    } 
    else {
        return Err("create_contiguous_mapping(): Couldn't get kernel's active_table");
    }
}


/// An area of physical memory. 
#[derive(Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct PhysicalMemoryArea {
    pub base_addr: PhysicalAddress,
    pub size_in_bytes: usize,
    pub typ: u32,
    pub acpi: u32
}
impl PhysicalMemoryArea {
    pub fn new(paddr: PhysicalAddress, size_in_bytes: usize, typ: u32, acpi: u32) -> PhysicalMemoryArea {
        PhysicalMemoryArea {
            base_addr: paddr,
            size_in_bytes: size_in_bytes,
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



/// A region of virtual memory that is mapped into a [`Task`](../task/struct.Task.html)'s address space
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
            return PageIter::empty();
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



static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(Vec<VirtualAddress>)> = Once::new();

/// Set the function callback that will be invoked every time a TLB shootdown is necessary,
/// i.e., during page table remapping and unmapping operations.
pub fn set_broadcast_tlb_shootdown_cb(func: fn(Vec<VirtualAddress>)) {
    BROADCAST_TLB_SHOOTDOWN_FUNC.call_once(|| func);
}



/// initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessibl.e
/// The returned MemoryManagementInfo struct is partially initialized with the kernel's StackAllocator instance, 
/// and the list of `VirtualMemoryArea`s that represent some of the kernel's mapped sections (for task zero).
pub fn init(boot_info: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    assert_has_not_been_called!("memory::init must be called only once");
    // let rsdt_phys_addr = boot_info.acpi_old_tag().and_then(|acpi| acpi.get_rsdp().map(|rsdp| rsdp.rsdt_phys_addr()));
    // debug!("rsdt_phys_addr: {:#X}", if let Some(pa) = rsdt_phys_addr { pa } else { 0 });
    
    let memory_map_tag = try!(boot_info.memory_map_tag().ok_or("Memory map tag not found"));
    let elf_sections_tag = try!(boot_info.elf_sections_tag().ok_or("Elf sections tag not found"));

    // Our linker script specifies that the kernel will have the .init section starting at 1MB and ending at 1MB + .init size
    // and all other kernel sections will start at (KERNEL_OFFSET + 1MB) and end at (KERNEL_OFFSET + 1MB + size).
    // So, the start of the kernel is its physical address, but the end of it is its virtual address... confusing, I know
    // Thus, kernel_phys_start is the same as kernel_virt_start initially, but we remap them later in paging::init.
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
  
    // parse the list of physical memory areas from multiboot
    let mut available: [PhysicalMemoryArea; 32] = Default::default();
    let mut avail_index = 0;
    for area in memory_map_tag.memory_areas() {
        debug!("memory area base_addr={:#x} length={:#x} ({:?})", area.start_address(), area.size(), area);
        
        // optimization: we reserve memory from areas below the end of the kernel's physical address,
        // which includes addresses beneath 1 MB
        if area.end_address() < kernel_phys_end {
            debug!("--> skipping region before kernel_phys_end");
            continue;
        }
        let start_paddr: PhysicalAddress = if area.start_address() >= kernel_phys_end { area.start_address() } else { kernel_phys_end };
        let start_paddr = (Frame::containing_address(start_paddr) + 1).start_address(); // align up to next page

        available[avail_index] = PhysicalMemoryArea {
            base_addr: start_paddr,
            size_in_bytes: area.end_address() - start_paddr,
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
    occupied[occup_index] = PhysicalMemoryArea::new(0, 0x10_0000, 1, 0); // reserve addresses under 1 MB
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(kernel_phys_start, kernel_phys_end-kernel_phys_start, 1, 0); // the kernel boot image is already in use
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(boot_info.start_address() - KERNEL_OFFSET, boot_info.end_address()-boot_info.start_address(), 1, 0); // preserve bootloader info
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(modules_start, modules_end - modules_start, 1, 0); // preserve all modules
    occup_index += 1;


    // init the frame allocator with the available memory sections and the occupied memory sections
    let fa = try!( AreaFrameAllocator::new(available, avail_index, occupied, occup_index));
    let frame_allocator_mutex: &MutexIrqSafe<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new( fa ) 
    });

    // print_early!("Boot info: {:?}\n", boot_info);


    // Initialize paging (create a new page table), which also initializes the kernel heap.
    let (active_table, kernel_vmas, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages) = try!(
        paging::init(frame_allocator_mutex, &boot_info)
    );
    // HERE: heap is initialized! Can now use alloc types.

    debug!("Done with paging::init()!, active_table: {:?}", active_table);
    // print_early!("Done with paging::init()!, active_table: {:?}\n", active_table);

    
    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start = Page::containing_address(KERNEL_STACK_ALLOCATOR_BOTTOM); 
        let stack_alloc_end = Page::containing_address(KERNEL_STACK_ALLOCATOR_TOP_ADDR);
        let stack_alloc_range = Page::range_inclusive(stack_alloc_start, stack_alloc_end);
        stack_allocator::StackAllocator::new(stack_alloc_range, false)
    };

    // return the kernel's memory info 
    let kernel_mmi = MemoryManagementInfo {
        page_table: PageTable::Active(active_table),
        vmas: kernel_vmas,
        extra_mapped_pages: higher_half_mapped_pages,
        stack_allocator: kernel_stack_allocator, 
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok( (kernel_mmi_ref.clone(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) )
}



/// A `Frame` is a chunk of **physical** memory, similar to how a `Page` is a chunk of **virtual** memory. 
/// Frames do not implement Clone or Copy because they cannot be safely duplicated 
/// (you cannot simply "copy" a region of physical memory...).
/// A `Frame` is the sole owner of the region of physical memory that it covers, 
/// i.e., there will never be two `Frame` objects that point to the same physical memory chunk. 
/// 
/// **Note**: DO NOT implement Copy or Clone for this type.
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

    pub fn range_inclusive(start: Frame, end: Frame) -> FrameIter {
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
        assert!(self.number < MAX_PAGE_NUMBER, "Frame addition error, cannot go above MAX_PAGE_NUMBER 0x000F_FFFF_FFFF_FFFF!");
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

#[derive(Debug)]
pub struct FrameIter {
    pub start: Frame,
    pub end: Frame,
}

impl FrameIter {
    pub fn start_address(&self) -> PhysicalAddress {
        self.start.start_address()
    }

    /// Create a duplicate of this FrameIter. 
    /// We do this instead of implementing/deriving the Clone trait
    /// because we want to prevent Rust from cloning `FrameIter`s implicitly.
    pub fn clone(&self) -> FrameIter {
        FrameIter {
            start: self.start.clone(),
            end: self.end.clone(),
        }
    }
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
    fn allocate_frames(&mut self, num_frames: usize) -> Option<FrameIter>;
    fn deallocate_frame(&mut self, frame: Frame);
    /// Call this when a heap is set up, and the `alloc` types can be used.
    fn alloc_ready(&mut self);
}

