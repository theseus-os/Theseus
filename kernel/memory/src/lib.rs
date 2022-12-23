//! This crate implements the main memory management subsystem for Theseus.
//!
//! The primary type of interest is [`MappedPages`], which offers a robust
//! interface that unifies the usage of arbitrary memory regions
//! with that of Rust's safe type system and lifetimes.
//!
//! ## Acknowledgments
//! Some of the internal page table management code was based on
//! Philipp Oppermann's [blog_os], but has since changed significantly.
//!
//! [blog_os]: https://github.com/phil-opp/blog_os

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]
#![feature(result_option_inspect)]

extern crate alloc;

mod paging;
pub use self::paging::{
    PageTable, Mapper, Mutability, Mutable, Immutable,
    MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
};

pub use memory_structs::{Frame, Page, FrameRange, PageRange, VirtualAddress, PhysicalAddress};
pub use page_allocator::{
    AllocatedPages, allocate_pages, allocate_pages_at,
    allocate_pages_by_bytes, allocate_pages_by_bytes_at,
};

pub use frame_allocator::{
    AllocatedFrames, MemoryRegionType, PhysicalMemoryRegion,
    allocate_frames, allocate_frames_at, allocate_frames_by_bytes_at, allocate_frames_by_bytes,
};

#[cfg(target_arch = "x86_64")]
use memory_x86_64::{
    find_section_memory_bounds, get_vga_mem_addr, tlb_flush_virt_addr, tlb_flush_all, get_p4,
};

pub use pte_flags::*;

use boot_info::{BootInformation, MemoryRegion};
use log::debug;
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use no_drop::NoDrop;
use kernel_config::memory::KERNEL_OFFSET;
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
pub fn create_contiguous_mapping<F: Into<PteFlagsArch>>(
    size_in_bytes: usize,
    flags: F,
) -> Result<(MappedPages, PhysicalAddress), &'static str> {
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
pub fn create_mapping<F: Into<PteFlagsArch>>(
    size_in_bytes: usize,
    flags: F,
) -> Result<MappedPages, &'static str> {
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
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  1. the kernel's new `PageTable`, which is now currently active,
///  2. the `MappedPages` of the kernel's text section,
///  3. the `MappedPages` of the kernel's rodata section,
///  4. the `MappedPages` of the kernel's data section,
///  5. a tuple of the stack's underlying guard page (an `AllocatedPages` instance) and the actual `MappedPages` backing it,
///  6. the `MappedPages` holding the bootloader info,
///  7. the kernel's list of *other* higher-half MappedPages that needs to be converted to a vector after heap initialization, and which should be kept forever,
///  8. the kernel's list of identity-mapped MappedPages that needs to be converted to a vector after heap initialization, and which should be dropped before starting the first userspace program. 
pub fn init(
    boot_info: &impl BootInformation,
    kernel_stack_start: VirtualAddress,
) -> Result<(
    PageTable,
    NoDrop<MappedPages>,
    NoDrop<MappedPages>,
    NoDrop<MappedPages>,
    (AllocatedPages, NoDrop<MappedPages>),
    MappedPages,
    [Option<NoDrop<MappedPages>>; 32],
    [Option<NoDrop<MappedPages>>; 32],
), &'static str> {
    // Get the start and end addresses of the kernel, boot info, boot modules, etc.
    // These are all physical addresses.
    let kernel_memory = boot_info.kernel_memory_range()?;
    let boot_info_memory = boot_info.bootloader_info_memory_range()?;
    let modules_memory = boot_info.modules_memory_range()?;

    debug!("kernel memory: p{:#X} to p{:#X}", kernel_memory.start, kernel_memory.end);
    debug!("boot info memory: p{:#X} to p{:#X}", boot_info_memory.start, boot_info_memory.end);
    debug!("modules memory: p{:#X} to p{:#X}", modules_memory.start, modules_memory.end);

    // In addition to the information about the hardware's physical memory map provided by the bootloader,
    // Theseus chooses to reserve the following regions of physical memory for specific use.
    let low_memory_frames   = FrameRange::from_phys_addr(PhysicalAddress::zero(), 0x10_0000); // suggested by most OS developers
    let kernel_frames       = FrameRange::from_phys_addr(kernel_memory.start, kernel_memory.end.value() - kernel_memory.start.value());
    let boot_modules_frames = FrameRange::from_phys_addr(modules_memory.start, modules_memory.end.value() - modules_memory.start.value());
    let boot_info_frames    = FrameRange::from_phys_addr(boot_info_memory.start, boot_info_memory.end.value() - boot_info_memory.start.value());
    
    // Add the VGA display's memory region to the list of reserved physical memory areas.
    // Currently this is covered by the first 1MiB region, but it's okay to duplicate it here.
    let (vga_start_paddr, vga_size, _vga_flags) = memory_x86_64::get_vga_mem_addr()?;
    let vga_display_frames = FrameRange::from_phys_addr(vga_start_paddr, vga_size);
    
    // Now set up the list of free regions and reserved regions so we can initialize the frame allocator.
    let mut free_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut free_index = 0;
    let mut reserved_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut reserved_index = 0;

    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(low_memory_frames, MemoryRegionType::Reserved));
    reserved_index += 1;
    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(kernel_frames, MemoryRegionType::Reserved));
    reserved_index += 1;
    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(boot_modules_frames, MemoryRegionType::Reserved));
    reserved_index += 1;
    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(boot_info_frames, MemoryRegionType::Reserved));
    reserved_index += 1;
    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(vga_display_frames, MemoryRegionType::Reserved));
    reserved_index += 1;

    for area in boot_info.memory_regions()? {
        let frames = FrameRange::from_phys_addr(PhysicalAddress::new_canonical(area.start().value()), area.len());
        if area.is_usable() {
            free_regions[free_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Free));
            free_index += 1;
        } else {
            reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Reserved));
            reserved_index += 1;
        }
    }

    let into_alloc_frames_fn = frame_allocator::init(free_regions.iter().flatten(), reserved_regions.iter().flatten())?;
    debug!("Initialized new frame allocator!");
    frame_allocator::dump_frame_allocator_state();

    page_allocator::init(VirtualAddress::new_canonical(kernel_memory.end.value()))?;
    debug!("Initialized new page allocator!");
    page_allocator::dump_page_allocator_state();

    // Initialize paging, which creates a new page table and maps all of the current code/data sections into it.
    paging::init(boot_info, kernel_stack_start, into_alloc_frames_fn)
        .inspect(|(new_page_table, ..)| {
            debug!("Done with paging::init(). new page table: {:?}", new_page_table);
        })
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
