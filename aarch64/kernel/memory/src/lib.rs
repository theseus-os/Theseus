//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]
#![feature(result_option_inspect)]

extern crate spin;
extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate bit_field;
extern crate memory_structs;
extern crate page_allocator;
extern crate frame_allocator;
extern crate page_table_entry;
extern crate memory_aarch64;
extern crate zerocopy;
extern crate no_drop;


#[cfg(not(mapper_spillful))]
mod paging;

#[cfg(mapper_spillful)]
pub mod paging;

pub use self::paging::{
    PageTable, Mapper, Mutability, Mutable, Immutable,
    MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
};

pub use memory_structs::{Frame, Page, FrameRange, PageRange, VirtualAddress, PhysicalAddress, PteFlags};
pub use page_allocator::{AllocatedPages, allocate_pages, allocate_pages_at,
    allocate_pages_by_bytes, allocate_pages_by_bytes_at};

pub use frame_allocator::{AllocatedFrames, MemoryRegionType, PhysicalMemoryRegion,
    allocate_frames_by_bytes_at, allocate_frames_by_bytes, allocate_frames_at};

#[cfg(target_arch = "x86_64")]
use memory_x86_64::{BootInformation, get_kernel_address, get_boot_info_mem_area, find_section_memory_bounds,
    get_vga_mem_addr, get_modules_address, tlb_flush_virt_addr, tlb_flush_all, get_p4};

#[cfg(target_arch = "aarch64")]
use memory_aarch64::{tlb_flush_virt_addr, tlb_flush_all, get_p4, set_page_table_up};

use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use no_drop::NoDrop;
pub use kernel_config::memory::PAGE_SIZE;

/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<MmiRef> = Once::new();

/// A shareable reference to a `MemoryManagementInfo` struct wrapper in a lock.
pub type MmiRef = Arc<MutexIrqSafe<MemoryManagementInfo>>;

/// Returns a reference to the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns `None`.
pub fn get_kernel_mmi_ref() -> Option<&'static MmiRef> {
    KERNEL_MMI.get()
}


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
#[derive(Debug)]
pub struct MemoryManagementInfo {
    /// the PageTable that should be switched to when this Task is switched to.
    pub page_table: PageTable,
    
    /// a list of additional virtual-mapped Pages that have the same lifetime as this MMI
    /// and are thus owned by this MMI, but is not all-inclusive (e.g., Stacks are excluded).
    pub extra_mapped_pages: Vec<MappedPages>,
}


/// A convenience function that creates a new memory mapping by allocating frames that are contiguous in physical memory.
/// If contiguous frames are not required, then see [`create_mapping()`](fn.create_mapping.html).
/// Returns a tuple containing the new `MappedPages` and the starting PhysicalAddress of the first frame,
/// which is a convenient way to get the physical address without walking the page tables.
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the frame allocator and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn create_contiguous_mapping(size_in_bytes: usize, flags: PteFlags) -> Result<(MappedPages, PhysicalAddress), &'static str> {
    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_contiguous_mapping(): couldn't allocate contiguous pages!")?;
    let allocated_frames = allocate_frames_by_bytes(size_in_bytes).ok_or("memory::create_contiguous_mapping(): couldn't allocate contiguous frames!")?;
    let starting_phys_addr = allocated_frames.start_address();
    let mp = kernel_mmi_ref.lock().page_table.map_allocated_pages_to(allocated_pages, allocated_frames, flags)?;
    Ok((mp, starting_phys_addr))
}


/// A convenience function that creates a new memory mapping. The pages allocated are contiguous in memory but there's
/// no guarantee that the frames they are mapped to are also contiguous in memory. If contiguous frames are required
/// then see [`create_contiguous_mapping()`](fn.create_contiguous_mapping.html).
/// Returns the new `MappedPages.` 
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that lock is not held when invoking this function.
pub fn create_mapping(size_in_bytes: usize, flags: PteFlags) -> Result<MappedPages, &'static str> {
    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_mapping(): couldn't allocate pages!")?;
    kernel_mmi_ref.lock().page_table.map_allocated_pages(allocated_pages, flags)
}


static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(PageRange)> = Once::new();

/// Set the function callback that will be invoked every time a TLB shootdown is necessary,
/// i.e., during page table remapping and unmapping operations.
pub fn set_broadcast_tlb_shootdown_cb(func: fn(PageRange)) {
    BROADCAST_TLB_SHOOTDOWN_FUNC.call_once(|| func);
}


/// Initializes the virtual memory management system.
/// 
/// Returns the kernel's current PageTable, if successful.
pub fn init(
    free_regions: &[Option<PhysicalMemoryRegion>; 32],
    reserved_regions: &[Option<PhysicalMemoryRegion>; 32],
) -> Result<PageTable, &'static str> {
    let into_alloc_frames_fn = frame_allocator::init(free_regions.iter().flatten(), reserved_regions.iter().flatten())?;
    debug!("Initialized new frame allocator!");
    frame_allocator::dump_frame_allocator_state();

    // On x86_64 `page_allocator` is initialized with a value
    // obtained from the ELF layout.
    // Here I'm choosing a value which is probably valid (uneducated guess);
    // once we have an ELF aarch64 kernel we'll be able to use the original
    // limit defined with KERNEL_OFFSET and the ELF layout.
    page_allocator::init(VirtualAddress::new_canonical(0x100_000_000))?;
    debug!("Initialized new page allocator!");
    page_allocator::dump_page_allocator_state();

    // Initialize paging, which only bootstraps the current page table at the moment.
    paging::init(into_alloc_frames_fn)
        .inspect(|page_table| debug!("Done with paging::init(). page table: {:?}", page_table))
}

/// Finishes initializing the memory management system after the heap is ready.
/// 
/// Returns the following tuple:
///  * The kernel's new [`MemoryManagementInfo`], representing the initial virtual address space,
///  * The kernel's list of identity-mapped [`MappedPages`],
///    which must not be dropped until all AP (additional CPUs) are fully booted,
///    but *should* be dropped before starting the first user application. 
pub fn init_post_heap(
    page_table: PageTable,
    mut higher_half_mapped_pages: [Option<NoDrop<MappedPages>>; 32],
    mut identity_mapped_pages: [Option<NoDrop<MappedPages>>; 32],
    heap_mapped_pages: MappedPages
) -> (MmiRef, NoDrop<Vec<MappedPages>>) {
    // HERE: heap is initialized! We can now use `alloc` types.

    page_allocator::convert_to_heap_allocated();
    frame_allocator::convert_to_heap_allocated();

    let mut higher_half_mapped_pages: Vec<MappedPages> = higher_half_mapped_pages
        .iter_mut()
        .filter_map(|opt| opt.take().map(NoDrop::into_inner))
        .collect();
    higher_half_mapped_pages.push(heap_mapped_pages);
    let identity_mapped_pages: Vec<MappedPages> = identity_mapped_pages
        .iter_mut()
        .filter_map(|opt| opt.take().map(NoDrop::into_inner))
        .collect();
    let identity_mapped_pages = NoDrop::new(identity_mapped_pages);
   
    // Construct the kernel's memory mgmt info, i.e., its address space info
    let kernel_mmi = MemoryManagementInfo {
        page_table,
        extra_mapped_pages: higher_half_mapped_pages,
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    (kernel_mmi_ref.clone(), identity_mapped_pages)
}
