//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

extern crate spin;
extern crate multiboot2;
extern crate alloc;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate xmas_elf;
extern crate bit_field;
#[cfg(target_arch = "x86_64")]
extern crate memory_x86_64;
extern crate x86_64;
extern crate memory_structs;
extern crate page_table_entry;
extern crate page_allocator;
extern crate frame_allocator;
extern crate zerocopy;


#[cfg(not(mapper_spillful))]
mod paging;

#[cfg(mapper_spillful)]
pub mod paging;


pub use self::paging::*;

pub use memory_structs::*;
pub use page_allocator::*;
pub use frame_allocator::*;

#[cfg(target_arch = "x86_64")]
use memory_x86_64::*;

#[cfg(target_arch = "x86_64")]
pub use memory_x86_64::EntryFlags;// Export EntryFlags so that others does not need to get access to memory_<arch>.

use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::KERNEL_OFFSET;
pub use kernel_config::memory::PAGE_SIZE;

/// The memory management info and address space of the kernel
static KERNEL_MMI: Once<MmiRef> = Once::new();

/// A shareable reference to a `MemoryManagementInfo` struct wrapper in a lock.
pub type MmiRef = Arc<MutexIrqSafe<MemoryManagementInfo>>;

/// Returns a cloned reference to the kernel's `MemoryManagementInfo`, if initialized.
/// If not, it returns None.
pub fn get_kernel_mmi_ref() -> Option<MmiRef> {
    KERNEL_MMI.get().cloned()
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
pub fn create_contiguous_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<(MappedPages, PhysicalAddress), &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_contiguous_mapping(): couldn't allocate contiguous pages!")?;
    let allocated_frames = allocate_frames_by_bytes(size_in_bytes).ok_or("memory::create_contiguous_mapping(): couldn't allocate contiguous frames!")?;

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let starting_phys_addr = allocated_frames.start_address();
    let mp = kernel_mmi.page_table.map_allocated_pages_to(allocated_pages, allocated_frames, flags)?;
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
pub fn create_mapping(size_in_bytes: usize, flags: EntryFlags) -> Result<MappedPages, &'static str> {
    let allocated_pages = allocate_pages_by_bytes(size_in_bytes).ok_or("memory::create_mapping(): couldn't allocate pages!")?;
    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_contiguous_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();
    kernel_mmi.page_table.map_allocated_pages(allocated_pages, flags)
}


pub static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(PageRange)> = Once::new();

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
    boot_info: &BootInformation
) -> Result<(
    PageTable,
    MappedPages,
    MappedPages,
    MappedPages,
    (AllocatedPages, MappedPages),
    MappedPages,
    [Option<MappedPages>; 32],
    [Option<MappedPages>; 32]
), &'static str> {
    // Get the start and end addresses of the kernel, boot info, boot modules, etc.
    let (kernel_phys_start, kernel_phys_end, kernel_virt_end) = get_kernel_address(&boot_info)?;
    let (boot_info_paddr_start, boot_info_paddr_end) = get_boot_info_mem_area(&boot_info)?;
    let (modules_start_paddr, modules_end_paddr) = get_modules_address(&boot_info);
    debug!("bootloader info memory: p{:#X} to p{:#X}, bootloader modules: p{:#X} to p{:#X}", 
        boot_info_paddr_start, boot_info_paddr_end, modules_start_paddr, modules_end_paddr,
    );
    debug!("kernel_phys_start: p{:#X}, kernel_phys_end: p{:#X} kernel_virt_end = v{:#x}",
        kernel_phys_start, kernel_phys_end, kernel_virt_end
    );

    // In addition to the information about the hardware's physical memory map provided by the bootloader,
    // Theseus chooses to reserve the following regions of physical memory for specific use.
    let low_memory_frames   = FrameRange::from_phys_addr(PhysicalAddress::zero(), 0x10_0000); // suggested by most OS developers
    let kernel_frames       = FrameRange::from_phys_addr(kernel_phys_start, kernel_phys_end.value() - kernel_phys_start.value());
    let boot_modules_frames = FrameRange::from_phys_addr(modules_start_paddr, modules_end_paddr.value() - modules_start_paddr.value());
    let boot_info_frames    = FrameRange::from_phys_addr(boot_info_paddr_start, boot_info_paddr_end.value() - boot_info_paddr_start.value());
    
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

    for area in boot_info.memory_map_tag()
        .ok_or("Multiboot2 boot information has no physical memory map information")?
        .all_memory_areas()
    {
        let frames = FrameRange::from_phys_addr(PhysicalAddress::new_canonical(area.start_address() as usize), area.size() as usize);
        if area.typ() == multiboot2::MemoryAreaType::Available {
            free_regions[free_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Free));
            free_index += 1;
        } else {
            reserved_regions[reserved_index] = Some(PhysicalMemoryRegion::new(frames, MemoryRegionType::Reserved));
            reserved_index += 1;
        }
    }

    frame_allocator::init(free_regions.iter().flatten(), reserved_regions.iter().flatten())?;
    debug!("Initialized new frame allocator!");
    frame_allocator::dump_frame_allocator_state();

    page_allocator::init(VirtualAddress::new_canonical(kernel_phys_end.value()))?;
    debug!("Initialized new page allocator!");
    page_allocator::dump_page_allocator_state();

    // Initialize paging, which creates a new page table and maps all of the current code/data sections into it.
    let (
        page_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        (stack_guard_page, stack_pages),
        boot_info_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ) = paging::init(boot_info)?;

    debug!("Done with paging::init(). new page_table: {:?}", page_table);
    Ok((
        page_table,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        (stack_guard_page, stack_pages),
        boot_info_pages,
        higher_half_mapped_pages,
        identity_mapped_pages
    ))
}

/// Finishes initializing the virtual memory management system after the heap is initialized and returns a MemoryManagementInfo instance,
/// which represents the initial (the kernel's) address space. 
/// 
/// Returns the following tuple, if successful:
///  * The kernel's new MemoryManagementInfo
///  * The kernel's list of identity-mapped MappedPages which should be dropped before starting the first userspace program. 
pub fn init_post_heap(page_table: PageTable, mut higher_half_mapped_pages: [Option<MappedPages>; 32], mut identity_mapped_pages: [Option<MappedPages>; 32], heap_mapped_pages: MappedPages) 
-> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, Vec<MappedPages>), &'static str> 
{
    // HERE: heap is initialized! Can now use alloc types.
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.  

    page_allocator::convert_to_heap_allocated();
    frame_allocator::convert_to_heap_allocated();

    let mut higher_half_mapped_pages: Vec<MappedPages> = higher_half_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
    higher_half_mapped_pages.push(heap_mapped_pages);
    let identity_mapped_pages: Vec<MappedPages> = identity_mapped_pages.iter_mut().filter_map(|opt| opt.take()).collect();
   
    // return the kernel's memory info 
    let kernel_mmi = MemoryManagementInfo {
        page_table: page_table,
        extra_mapped_pages: higher_half_mapped_pages,
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok( (kernel_mmi_ref.clone(), identity_mapped_pages) )
}
