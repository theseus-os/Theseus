//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(asm)]
#![feature(ptr_internals)]
#![feature(core_intrinsics)]
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
extern crate heap_irq_safe;
#[macro_use] extern crate derive_more;
extern crate bit_field;
extern crate type_name;
#[cfg(target_arch = "x86_64")]
extern crate memory_x86;
extern crate memory_area;


/// Just like Rust's `try!()` macro, 
/// but forgets the given `obj`s to prevent them from being dropped,
/// as they would normally be upon return of an Error using `try!()`.
/// This must come BEFORE the below modules in order for them to be able to use it.
#[macro_export]
macro_rules! try_forget {
    ($expr:expr, $($obj:expr),*) => (match $expr {
        Ok(val) => val,
        Err(err) => {
            $(
                core::mem::forget($obj);
            )*
            return Err(err);
        }
    });
}


mod area_frame_allocator;
mod paging;
mod stack_allocator;


pub use self::area_frame_allocator::AreaFrameAllocator;
pub use self::paging::*;
pub use self::stack_allocator::{StackAllocator, Stack};

pub use memory_area::{VirtualAddress, PhysicalAddress, PhysicalMemoryArea, Frame, VirtualMemoryArea, Page, PageRange};

#[cfg(target_arch = "x86_64")]
use memory_x86::{set_new_p4, get_p4_address, get_kernel_address, get_available_memory, get_modules_address, get_boot_info_mem_area, get_boot_info_address, add_sections_vmem_areas, add_vga_vmem_area, tlb, BootInformation};

#[cfg(target_arch = "x86_64")]
pub use memory_x86::EntryFlags;// Export EntryFlags so that others does not need to get access to memory_<arch>.


use core::{
    ops::{RangeInclusive, Deref, DerefMut},
    iter::Step,
    mem,
};
use spin::Once;
use irq_safety::MutexIrqSafe;
use alloc::vec::Vec;
use alloc::sync::Arc;
use kernel_config::memory::{PAGE_SIZE, MAX_PAGE_NUMBER, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_STACK_ALLOCATOR_BOTTOM, KERNEL_STACK_ALLOCATOR_TOP_ADDR, KERNEL_HEAP_P4_INDEX, KERNEL_STACK_P4_INDEX, KERNEL_TEXT_P4_INDEX, KERNEL_OFFSET};
use bit_field::BitField;

/// The memory management info and address space of the kernel
pub static KERNEL_MMI: Once<Arc<MutexIrqSafe<MemoryManagementInfo>>> = Once::new();

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

    /// Allocates a new stack in the currently-running Task's address space.
    /// Also, this adds the newly-allocated stack to this struct's `vmas` vector. 
    /// Whether this is a kernelspace or userspace stack is determined by how this MMI's stack_allocator was initialized.
    /// 
    /// # Important Note
    /// You cannot call this to allocate a stack in a different `MemoryManagementInfo`/`PageTable` than the one you're currently running. 
    /// It will only work for allocating a stack in the currently-running MMI.
    pub fn alloc_stack(&mut self, size_in_pages: usize) -> Option<Stack> {
        let &mut MemoryManagementInfo { ref mut page_table, ref mut vmas, ref mut stack_allocator, .. } = self;
    
        if let Some( (stack, stack_vma) ) = FRAME_ALLOCATOR.try().and_then(|fa| stack_allocator.alloc_stack(page_table, fa.lock().deref_mut(), size_in_pages)) {
            vmas.push(stack_vma);
            Some(stack)
        }
        else {
            error!("MemoryManagementInfo::alloc_stack: failed to allocate stack of {} pages!", size_in_pages);
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

    let mut frame_allocator = FRAME_ALLOCATOR.try()
        .ok_or("create_contiguous_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();
    let frames = frame_allocator.allocate_frames(allocated_pages.size_in_pages())
        .ok_or("create_contiguous_mapping(): couldnt allocate a new frame")?;
    let starting_phys_addr = frames.start_address();
    let mp = kernel_mmi.page_table.map_allocated_pages_to(allocated_pages, frames, flags, frame_allocator.deref_mut())?;
    Ok((mp, starting_phys_addr))
}



static BROADCAST_TLB_SHOOTDOWN_FUNC: Once<fn(Vec<VirtualAddress>)> = Once::new();

/// Set the function callback that will be invoked every time a TLB shootdown is necessary,
/// i.e., during page table remapping and unmapping operations.
pub fn set_broadcast_tlb_shootdown_cb(func: fn(Vec<VirtualAddress>)) {
    BROADCAST_TLB_SHOOTDOWN_FUNC.call_once(|| func);
}

/// Initializes the virtual memory management system and returns a MemoryManagementInfo instance,
/// which represents Task zero's (the kernel's) address space. 
/// Consumes the given BootInformation, because after the memory system is initialized,
/// the original BootInformation will be unmapped and inaccessible.
/// 
/// Returns the following tuple, if successful:
///  * The kernel's new MemoryManagementInfo
///  * the MappedPages of the kernel's text section,
///  * the MappedPages of the kernel's rodata section,
///  * the MappedPages of the kernel's data section,
///  * the kernel's list of *other* higher-half MappedPages, which should be kept forever.
pub fn init(boot_info: &BootInformation) 
    -> Result<(Arc<MutexIrqSafe<MemoryManagementInfo>>, MappedPages, MappedPages, MappedPages, Vec<MappedPages>), &'static str> 
{
    // Get the start and end address of the kernel.
    let (kernel_phys_start, kernel_phys_end, kernel_virt_end, memory_map_tag) = get_kernel_address(&boot_info)?;

    debug!("kernel_phys_start: {:#x}, kernel_phys_end: {:#x} kernel_virt_end = {:#x}",
        kernel_phys_start,
        kernel_phys_end,
        kernel_virt_end
    );
  
    // Get availabe physical memory areas
    let (available, avail_len) = get_available_memory(memory_map_tag, kernel_phys_end)?;

    // Get the address of memory occupied by loaded modules
    // (we can reclaim this later after the module is loaded, but not until then).
    let (modules_start, modules_end) = get_modules_address(&boot_info);

    // print_early!("Modules physical memory region: start {:#X} to end {:#X}", modules_start, modules_end);

    let mut occupied: [PhysicalMemoryArea; 32] = Default::default();
    let mut occup_index = 0;
    
    
    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::zero(), 0x10_0000, 1, 0); // reserve addresses under 1 MB
    occup_index += 1;
    occupied[occup_index] = PhysicalMemoryArea::new(kernel_phys_start, kernel_phys_end.value() - kernel_phys_start.value(), 1, 0); // the kernel boot image is already in use
    occup_index += 1;
    
    #[cfg(target_arch = "x86_64")]
    {   
        // preserve the multiboot information
        occupied[occup_index] = get_boot_info_mem_area(&boot_info)?;
        occup_index += 1;
    }

    occupied[occup_index] = PhysicalMemoryArea::new(PhysicalAddress::new(modules_start)?, modules_end - modules_start, 1, 0); // preserve all modules
    occup_index += 1;

    // init the frame allocator with the available memory sections and the occupied memory sections
    let fa = AreaFrameAllocator::new(available, avail_len, occupied, occup_index)?;
    let frame_allocator_mutex: &MutexIrqSafe<AreaFrameAllocator> = FRAME_ALLOCATOR.call_once(|| {
        MutexIrqSafe::new( fa ) 
    });

    // print_early!("Boot info: {:?}\n", boot_info);


    // Initialize paging (create a new page table), which also initializes the kernel heap.

    let (
        page_table,
        kernel_vmas,
        text_mapped_pages,
        rodata_mapped_pages,
        data_mapped_pages,
        higher_half_mapped_pages,
        identity_mapped_pages,
    ) = paging::init(frame_allocator_mutex, &boot_info)?;

    // HERE: heap is initialized! Can now use alloc types.
    // After this point, we must "forget" all of the above mapped_pages instances if an error occurs,
    // because they will be auto-unmapped from the new page table upon return, causing all execution to stop.   

    debug!("Done with paging::init()!, page_table: {:?}", page_table);

    
    // init the kernel stack allocator, a singleton
    let kernel_stack_allocator = {
        let stack_alloc_start = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_BOTTOM)); 
        let stack_alloc_end = Page::containing_address(VirtualAddress::new_canonical(KERNEL_STACK_ALLOCATOR_TOP_ADDR));
        let stack_alloc_range = PageRange::new(stack_alloc_start, stack_alloc_end);
        StackAllocator::new(stack_alloc_range, false)
    };

    // return the kernel's memory info 
    let kernel_mmi = MemoryManagementInfo {
        page_table: page_table,
        vmas: kernel_vmas,
        extra_mapped_pages: higher_half_mapped_pages,
        stack_allocator: kernel_stack_allocator, 
    };

    let kernel_mmi_ref = KERNEL_MMI.call_once( || {
        Arc::new(MutexIrqSafe::new(kernel_mmi))
    });

    Ok( (kernel_mmi_ref.clone(), text_mapped_pages, rodata_mapped_pages, data_mapped_pages, identity_mapped_pages) )
}



/// A range of `Frame`s that are contiguous in physical memory.
#[derive(Debug, Clone)]
pub struct FrameRange(RangeInclusive<Frame>);

impl FrameRange {
    /// Creates a new range of `Frame`s that spans from `start` to `end`,
    /// both inclusive bounds.
    pub fn new(start: Frame, end: Frame) -> FrameRange {
        FrameRange(RangeInclusive::new(start, end))
    }

    /// Creates a FrameRange that will always yield `None`.
    pub fn empty() -> FrameRange {
        FrameRange::new(Frame { number: 1 }, Frame { number: 0 })
    }
    
    /// A convenience method for creating a new `FrameRange` 
    /// that spans all `Frame`s from the given physical address 
    /// to an end bound based on the given size.
    pub fn from_phys_addr(starting_virt_addr: PhysicalAddress, size_in_bytes: usize) -> FrameRange {
        let start_frame = Frame::containing_address(starting_virt_addr);
        let end_frame = Frame::containing_address(starting_virt_addr + size_in_bytes - 1);
        FrameRange::new(start_frame, end_frame)
    }

    /// Returns the `PhysicalAddress` of the starting `Frame` in this `FrameRange`.
    pub fn start_address(&self) -> PhysicalAddress {
        self.0.start().start_address()
    }

    /// Returns the number of `Frame`s covered by this iterator. 
    /// Use this instead of the Iterator trait's `count()` method.
    /// This is instant, because it doesn't need to iterate over each entry, unlike normal iterators.
    pub fn size_in_frames(&self) -> usize {
        // add 1 because it's an inclusive range
        self.0.end().number + 1 - self.0.start().number
    }

    /// Whether this `FrameRange` contains the given `PhysicalAddress`.
    pub fn contains_phys_addr(&self, phys_addr: PhysicalAddress) -> bool {
        self.0.contains(&Frame::containing_address(phys_addr))
    }

    /// Returns the offset of the given `PhysicalAddress` within this `FrameRange`,
    /// i.e., the difference between `phys_addr` and `self.start()`.
    pub fn offset_from_start(&self, phys_addr: PhysicalAddress) -> Option<usize> {
        if self.contains_phys_addr(phys_addr) {
            Some(phys_addr.value() - self.start_address().value())
        } else {
            None
        }
    }

    /// Returns a new, separate `FrameRange` that is extended to include the given `Frame`. 
    pub fn to_extended(&self, frame_to_include: Frame) -> FrameRange {
        // if the current FrameRange was empty, return a new FrameRange containing only the given frame_to_include
        if self.is_empty() {
            return FrameRange::new(frame_to_include.clone(), frame_to_include);
        }

        let start = core::cmp::min(self.0.start(), &frame_to_include);
        let end   = core::cmp::max(self.0.end(),   &frame_to_include);
        FrameRange::new(start.clone(), end.clone())
    }
}

impl Deref for FrameRange {
    type Target = RangeInclusive<Frame>;
    fn deref(&self) -> &RangeInclusive<Frame> {
        &self.0
    }
}
impl DerefMut for FrameRange {
    fn deref_mut(&mut self) -> &mut RangeInclusive<Frame> {
        &mut self.0
    }
}

impl IntoIterator for FrameRange {
    type Item = Frame;
    type IntoIter = RangeInclusive<Frame>;

    fn into_iter(self) -> Self::IntoIter {
        self.0
    }
}


pub trait FrameAllocator {
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn allocate_frames(&mut self, num_frames: usize) -> Option<FrameRange>;
    fn deallocate_frame(&mut self, frame: Frame);
    /// Call this when a heap is set up, and the `alloc` types can be used.
    fn alloc_ready(&mut self);
}

