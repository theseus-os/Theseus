//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
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
extern crate bit_field;
#[cfg(target_arch = "x86_64")]
extern crate memory_x86_64;
extern crate memory_structs;


mod area_frame_allocator;
mod stack_allocator;
#[cfg(not(mapper_spillful))]
mod paging;

#[cfg(mapper_spillful)]
pub mod paging;


pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::*;
pub use self::stack_allocator::{StackAllocator, Stack};

pub use memory_structs::*;

#[cfg(target_arch = "x86_64")]
use memory_x86_64::*;

#[cfg(target_arch = "x86_64")]
pub use memory_x86_64::EntryFlags;// Export EntryFlags so that others does not need to get access to memory_<arch>.


use core::{
    ops::{Deref, DerefMut},
};
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR, KERNEL_OFFSET};

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
pub fn allocate_frames(num_frames: usize) -> Option<FrameRange> {
    FRAME_ALLOCATOR.try().and_then(|fa| fa.lock().allocate_frames(num_frames))
}

/// An Arc reference to a `MemoryManagementInfo` struct.
pub type MmiRef = Arc<MutexIrqSafe<MemoryManagementInfo>>;


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
#[derive(Debug)]
pub struct MemoryManagementInfo {
    /// the PageTable that should be switched to when this Task is switched to.
    pub page_table: PageTable,
    
    /// a list of additional virtual-mapped Pages that have the same lifetime as this MMI
    /// and are thus owned by this MMI, but is not all-inclusive (e.g., Stacks are excluded).
    pub extra_mapped_pages: Vec<MappedPages>,

    /// the task's stack allocator, which is initialized with a range of Pages from which to allocate.
    stack_allocator: stack_allocator::StackAllocator,
}

impl MemoryManagementInfo {

    /// Allocates a new stack in the currently-running Task's address space.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    /// Whether this is a kernelspace or userspace stack is determined by how this MMI's stack_allocator was initialized.
    /// 
    /// # Important Note
    /// You cannot call this to allocate a stack in a different `MemoryManagementInfo`/`PageTable` than the one you're currently running. 
    /// It will only work for allocating a stack in the currently-running MMI.
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        FRAME_ALLOCATOR.try().and_then(|fa| 
            self.stack_allocator.alloc_stack(&mut self.page_table, fa.lock().deref_mut(), size_in_pages)
        )
    }
}


/// A convenience function that creates a new memory mapping by allocating frames that are contiguous in physical memory.
/// If contiguous frames are not required, then see [`create_mapping()`](fn.create_mapping.html).
/// Returns a tuple containing the new `MappedPages` and the starting PhysicalAddress of the first frame,
/// which is a convenient way to get the physical address without walking the page tables.
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the `FRAME_ALLOCATOR` and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn create_contiguous_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<(MappedPages, PhysicalAddress), &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_contiguous_mapping(): couldn't allocate pages!")?;

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let mut frame_allocator = FRAME_ALLOCATOR.try()
        .ok_or("create_contiguous_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();
    let frames = frame_allocator.allocate_frames(allocated_pages.size_in_pages())
        .ok_or("create_contiguous_mapping(): couldnt allocate a new frame")?;
    let starting_phys_addr = frames.start_address();
    let mp = kernel_mmi.page_table.map_allocated_pages_to(allocated_pages, frames, flags, frame_allocator.deref_mut())?;
    Ok((mp, starting_phys_addr))
}


/// A convenience function that creates a new memory mapping. The pages allocated are contiguous in memory but there's
/// no guarantee that the frames they are mapped to are also contiguous in memory. If contiguous frames are required
/// then see [`create_contiguous_mapping()`](fn.create_contiguous_mapping.html).
/// Returns the new `MappedPages.` 
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the `FRAME_ALLOCATOR` and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn create_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<MappedPages, &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_mapping(): couldn't allocate pages!")?;

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let mut frame_allocator = FRAME_ALLOCATOR.try()
        .ok_or("create_contiguous_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();
    
    kernel_mmi.page_table.map_allocated_pages(allocated_pages, flags, frame_allocator.deref_mut())
}


pub static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(Vec<VirtualAddress>)> = Once::new();

/// Set the function callback that will be invoked every time a TLB shootdown is necessary,
/// i.e., during page table remapping and unmapping operations.
pub fn set_broadcast_tlb_shootdown_cb(func: fn(Vec<VirtualAddress>)) {
    BROADCAST_TLB_SHOOTDOWN_FUNC.call_once(|| func);
}



/// Initializes the virtual memory management system.
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  * a reference to the Area Frame Allocator to be used by the remaining memory init functions
///  * the kernel's new PageTable, which is now currently active 
///  * the MappedPages of the kernel's text section,
///  * the MappedPages of the kernel's rodata section,
///  * the MappedPages of the kernel's data section,
///  * the kernel's list of *other* higher-half MappedPages that needs to be converted to a vector after heap initialization, and which should be kept forever,
///  * the kernel's list of identity-mapped MappedPages that needs to be converted to a vector after heap initialization, and which should be dropped before starting the first userspace program. 
pub fn init(boot_info: &BootInformation) 
    -> Result<(&MutexIrqSafe<AreaFrameAllocator>, PageTable, MappedPages, MappedPages, MappedPages, [Option<MappedPages>; 32], [Option<MappedPages>; 32]), &'static str> 
{
    // get the start and end addresses of the kernel.
    let (kernel_phys_start, kernel_phys_end, kernel_virt_end) = get_kernel_address(&boot_info)?;

    debug!("kernel_phys_start: {:#x}, kernel_phys_end: {:#x} kernel_virt_end = {:#x}",
        kernel_phys_start,
        kernel_phys_end,
        kernel_virt_end
    );
  
    // get available physical memory areas
    let (available, avail_len) = get_available_memory(&boot_info, kernel_phys_end)?;

    // Get the bounds of physical memory that is occupied by bootloader-loaded modules.
    let (modules_start_paddr, modules_end_paddr) = get_modules_address(&boot_info);

    // Set up the initial list of reserved physical memory frames such that the frame allocator does not re-use them.
    let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
    let mut occup_index = 0;
    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::zero(), 0x10_0000, 1, 0); // reserve addresses under 1 MB
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(kernel_phys_start, kernel_phys_end.value() - kernel_phys_start.value(), 1, 0); // the kernel boot image is already in use
    occup_index += 1;
    occupied[occup_index] = get_boot_info_mem_area(&boot_info)?; // preserve the multiboot information for x86_64. 
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(modules_start_paddr, modules_end_paddr.value() - modules_start_paddr.value(), 1, 0); // preserve all bootloader modules
    occup_index += 1;


    // init the frame allocator with the available memory sections and the occupied memory sections
    let fa = AreaFrameAllocator::new(available, avail_len, occupied, occup_index)?;
    let frame_allocator_mutex: &MutexIrqSafe<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new( fa ) 
    });

    // Initialize paging (create a new page table).

    let (
        page_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ) = paging::init(frame_allocator_mutex, &boot_info)?;

    debug!("Done with paging::init()!, page_table: {:?}", page_table);

    Ok((frame_allocator_mutex, page_table, text_mapped_pages, rodata_mapped_pages, data_mapped_pages, higher_half_mapped_pages, identity_mapped_pages))
    
}

/// Finishes Initializing the virtual memory management system after the heap is initialized and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// 
/// Returns the following tuple, if successful:
///  * The kernel's new MemoryManagementInfo
///  * The kernel's list of identity-mapped MappedPages which should be dropped before starting the first userspace program. 
pub fn init_post_heap(page_table: PageTable, mut higher_half_mapped_pages: [Option<MappedPages>; 32], mut identity_mapped_pages: [Option<MappedPages>; 32]) 
-> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, Vec<MappedPages>), &'static str> 
{
    // HERE: heap is initialized! Can now use alloc types.
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.  

    let higher_half_mapped_pages: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    let identity_mapped_pages: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
   
    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_BOTTOM)); 
        let stack_alloc_end = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_TOP_ADDR));
        let stack_alloc_range = PageRange::new(stack_alloc_start, stack_alloc_end);
        stack_allocator::StackAllocator::new(stack_alloc_range, false)
    };

    // return the kernel's memory info 
    let kernel_mmi = MemoryManagementInfo {
        page_table: page_table,
        extra_mapped_pages: higher_half_mapped_pages,
        stack_allocator: kernel_stack_allocator, 
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok( (kernel_mmi_ref.clone(), identity_mapped_pages) )
}

pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn allocate_frames(&mut self, num_frames: usize) -> Option<FrameRange>;
    fn deallocate_frame(&mut self, frame: Frame);
    /// Call this when a heap is set up, and the `alloc` types can be used.
    fn alloc_ready(&mut self);
}

