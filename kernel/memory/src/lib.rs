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

extern crate alloc;

mod paging;
pub use self::paging::{
    PageTable, Mapper, Mutability, Mutable, Immutable,
    MappedPages, BorrowedMappedPages, BorrowedSliceMappedPages,
    translate,
};

pub use memory_structs::*;
pub use page_allocator::{
    AllocatedPages,
    AllocationRequest,
    allocate_pages,
    allocate_pages_at,
    allocate_pages_by_bytes,
    allocate_pages_by_bytes_at,
    allocate_pages_in_range,
    allocate_pages_by_bytes_in_range,
    dump_page_allocator_state,
};
pub use frame_allocator::{
    AllocatedFrames,
    UnmappedFrames,
    allocate_frames,
    allocate_frames_at,
    allocate_frames_by_bytes,
    allocate_frames_by_bytes_at,
    dump_frame_allocator_state,
};

#[cfg(target_arch = "x86_64")]
use memory_x86_64::{tlb_flush_virt_addr, tlb_flush_all, get_p4, find_section_memory_bounds, get_vga_mem_addr};

#[cfg(target_arch = "aarch64")]
use memory_aarch64::{tlb_flush_virt_addr, tlb_flush_all, get_p4, find_section_memory_bounds};

pub use pte_flags::*;

use boot_info::{BootInformation, MemoryRegion};
use log::debug;
use spin::Once;
use sync_irq::IrqSafeMutex;
use alloc::{sync::Arc, vec::Vec};
use frame_allocator::{PhysicalMemoryRegion, MemoryRegionType};
use no_drop::NoDrop;
pub use kernel_config::memory::PAGE_SIZE;

/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<MmiRef> = Once::new();

/// A shareable reference to a `MemoryManagementInfo` struct wrapper in a lock.
pub type MmiRef = Arc<IrqSafeMutex<MemoryManagementInfo>>;

/// Returns a reference to the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns `None`.
pub fn get_kernel_mmi_ref() -> Option<&'static MmiRef> {
    KERNEL_MMI.get()
}


/// This holds all the information for a `Task`'s memory mappings and address space
/// (this is basically the equivalent of Linux's mm_struct)
#[derive(Debug)]
#[doc(alias("mmi"))]
pub struct MemoryManagementInfo {
    /// the PageTable that should be switched to when this Task is switched to.
    pub page_table: PageTable,
    
    /// The list of additional memory mappings that have the same lifetime as this MMI
    /// and are thus owned by this MMI.
    /// This currently includes only the mappings for the heap and the early VGA buffer.
    pub extra_mapped_pages: Vec<MappedPages>,
}

/// Mapping flags that can be used to map MMIO registers.
pub const MMIO_FLAGS: PteFlags = PteFlags::from_bits_truncate(
    PteFlags::new().bits()
    | PteFlags::VALID.bits()
    | PteFlags::WRITABLE.bits()
    | PteFlags::DEVICE_MEMORY.bits()
);

/// Mapping flags that can be used to map DMA (Direct Memory Access) memory.
pub const DMA_FLAGS: PteFlags = PteFlags::from_bits_truncate(
    PteFlags::new().bits()
    | PteFlags::VALID.bits()
    | PteFlags::WRITABLE.bits()
);


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


/// A convenience function that maps randomly-allocated pages to the given range of frames.
/// 
/// # Locking / Deadlock
/// Currently, this function acquires the lock on the frame allocator and the kernel's `MemoryManagementInfo` instance.
/// Thus, the caller should ensure that the locks on those two variables are not held when invoking this function.
pub fn map_frame_range<F: Into<PteFlagsArch>>(
    start_address: PhysicalAddress,
    size_in_bytes: usize,
    flags: F,
) -> Result<MappedPages, &'static str> {
    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("map_range(): KERNEL_MMI was not yet initialized!")?;
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::map_range(): couldn't allocate contiguous pages!")?;
    let allocated_frames = allocate_frames_by_bytes_at(start_address, size_in_bytes)
        .map_err(|_| "memory::map_range(): couldn't allocate contiguous frames!")?;
    kernel_mmi_ref.lock().page_table.map_allocated_pages_to(allocated_pages, allocated_frames, flags)
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

/// Information returned after initialising the memory subsystem.
#[derive(Debug)]
pub struct InitialMemoryMappings {
    /// The currently active page table.
    pub page_table: PageTable,
    /// The kernel's `.text` section mappings, which includes `.init`.
    pub text: NoDrop<MappedPages>,
    /// The kernel's `.rodata` section mappings.
    pub rodata: NoDrop<MappedPages>,
    /// The kernel's .`data` section mappings/
    pub data: NoDrop<MappedPages>,
    /// The kernel stack's guard page.
    pub stack_guard: AllocatedPages,
    /// The kernel's stack actual data page mappings.
    pub stack: NoDrop<MappedPages>,
    /// The boot information mappings.
    pub boot_info: MappedPages,
    /// The list of identity mappings that should be dropped before starting the first application.
    ///
    /// Currently there are only 4 identity mappings, used for the base kernel image:
    /// 1. the `.init` early text section,
    /// 2. the full `.text` section,
    /// 3. the `.rodata` section, which includes all read-only data,
    /// 4. the `.data` section, which includes `.bss` and all read-write data.
    pub identity: NoDrop<EarlyIdentityMappedPages>,
    /// The list of additional mappings that must be kept forever.
    ///
    /// Currently, this contains only one mapping: the early VGA buffer.
    pub additional: NoDrop<MappedPages>,
}

/// The set of identity mappings that should be dropped before starting the first application.
/// 
/// Currently there are only 4 identity mappings, used for the base kernel image:
/// 1. the `.init` early text section,
/// 2. the full `.text` section,
/// 3. the `.rodata` section, which includes all read-only data,
/// 4. the `.data` section, which includes `.bss` and all read-write data.
#[derive(Debug)]
pub struct EarlyIdentityMappedPages {
    _init:   MappedPages,
    _text:   MappedPages,
    _rodata: MappedPages,
    _data:   MappedPages,
}

/// Initializes the virtual memory management system.
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
pub fn init(
    boot_info: &impl BootInformation,
    kernel_stack_start: VirtualAddress,
) -> Result<InitialMemoryMappings, &'static str> {
    let low_memory_frames   = FrameRange::from_phys_addr(PhysicalAddress::zero(), 0x10_0000); // suggested by most OS developers
    
    // Now set up the list of free regions and reserved regions so we can initialize the frame allocator.
    let mut free_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut free_index = 0;
    let mut reserved_regions: [Option<PhysicalMemoryRegion>; 32] = Default::default();
    let mut reserved_index = 0;

    reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(low_memory_frames, MemoryRegionType::Reserved));
    reserved_index += 1;

    #[cfg(target_arch = "x86_64")]
    {    
        // Add the VGA display's memory region to the list of reserved physical memory areas.
        // Currently this is covered by the first 1MiB region, but it's okay to duplicate it here.
        let (vga_start_paddr, vga_size, _vga_flags) = memory_x86_64::get_vga_mem_addr()?;
        let vga_display_frames = FrameRange::from_phys_addr(vga_start_paddr, vga_size);
        reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(vga_display_frames, MemoryRegionType::Reserved));
        reserved_index += 1;
    }

    for region in boot_info.memory_regions()? {
        let frames = FrameRange::from_phys_addr(region.start(), region.len());
        if region.is_usable() {
            free_regions[free_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Free));
            free_index += 1;
        } else {
            reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Reserved));
            reserved_index += 1;
        }
    }

    for region in boot_info.additional_reserved_memory_regions()? {
        reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(
            FrameRange::from_phys_addr(region.start, region.len),
            MemoryRegionType::Reserved,
        ));
        reserved_index += 1;
    }

    let into_alloc_frames_fn = frame_allocator::init(free_regions.iter().flatten(), reserved_regions.iter().flatten())?;
    debug!("Initialized new frame allocator!");
    frame_allocator::dump_frame_allocator_state();

    page_allocator::init(
        VirtualAddress::new(
            // We subtract 1 when translating because `kernel_end` returns an exclusive
            // upper bound, which can cause problems if the kernel ends on a page boundary.
            // We then add it back later to get the correct identity virtual address.
            translate(boot_info.kernel_end()? - 1)
                .ok_or("couldn't translate kernel end virtual address")?
                .value()
                + 1,
        )
        .ok_or("couldn't convert kernel end physical address into virtual address")?,
    )?;
    debug!("Initialized new page allocator!");
    page_allocator::dump_page_allocator_state();

    // Initialize paging, which creates a new page table and maps all of the current code/data sections into it.
    paging::init(boot_info, kernel_stack_start, into_alloc_frames_fn)
}

/// Finishes initializing the memory management system after the heap is ready.
/// 
/// Returns the following tuple:
///  * The kernel's new [`MemoryManagementInfo`], representing the initial virtual address space,
///  * The kernel's list of identity-mapped [`MappedPages`],
///    which must not be dropped until all secondary CPUs are fully booted,
///    but *should* be dropped before starting the first application.
pub fn init_post_heap(
    page_table: PageTable,
    additional_mapped_pages: MappedPages,
    heap_mapped_pages: MappedPages
) -> MmiRef {
    // HERE: heap is initialized! We can now use `alloc` types.

    page_allocator::convert_page_allocator_to_heap_based();
    frame_allocator::convert_frame_allocator_to_heap_based();

    let extra_mapped_pages = alloc::vec![additional_mapped_pages, heap_mapped_pages];
   
    // Construct the kernel's memory mgmt info, i.e., its address space info
    let kernel_mmi = MemoryManagementInfo {
        page_table,
        extra_mapped_pages,
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(IrqSafeMutex::new(kernel_mmi))
    });

    kernel_mmi_ref.clone()
}
