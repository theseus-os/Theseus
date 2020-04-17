//! The global allocator for the system. 
//! It starts off as a single fixed size allocator.
//! When a more complex heap is set up, it is set as the default allocator.
//! Any memory request greater than 8056 bytes is satisfied through a request for pages from the kernel.

#![feature(const_fn)]
#![feature(allocator_api)]
#![no_std]

extern crate alloc;
extern crate irq_safety; 
extern crate spin;
extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;

use core::ptr::{self, NonNull};
use alloc::alloc::{GlobalAlloc, Layout};
use memory::{EntryFlags, VirtualAddress, PageTable, AreaFrameAllocator, PageRange, create_mapping, MappedPages, FrameAllocator, FrameAllocatorRef};
use kernel_config::memory::PAGE_SIZE;
use irq_safety::MutexIrqSafe;
use core::ops::Add;
use spin::Once;
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use core::ops::DerefMut;
use alloc::boxed::Box;


#[global_allocator]
static ALLOCATOR: Heap = Heap::empty();

/// The heap mapped pages should be writable
pub const HEAP_FLAGS: EntryFlags = EntryFlags::WRITABLE;


/// Initializes the initial allocator, which is the first heap used by the system.
pub fn init_single_heap<A: FrameAllocator>(
    frame_allocator_ref: &FrameAllocatorRef<A>, page_table: &mut PageTable, start_virt_addr: usize, size_in_pages: usize
) -> Result<(), &'static str> {
 
    let mapped_pages_per_size_class =  size_in_pages / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * (ObjectPage8k::SIZE/ PAGE_SIZE));
    let mut heap_end_addr = VirtualAddress::new(start_virt_addr)?;
    let mut zone_allocator = ZoneAllocator::new();

    let alloc_sizes = &ZoneAllocator::BASE_ALLOC_SIZES;
    for size in alloc_sizes {
        for _ in 0..mapped_pages_per_size_class {
            // the alignment is equal to the size unless the size is not a multiple of 2
            let mut alignment = *size;
            if alignment == ZoneAllocator::MAX_BASE_ALLOC_SIZE {
                alignment = 8;
            }
            let layout = Layout::from_size_align(*size, alignment).map_err(|_e| "Incorrect layout")?;

            // create the mapped pages starting from the previous end of the heap
            let pages = PageRange::from_virt_addr(heap_end_addr, ObjectPage8k::SIZE);
            let mapping = page_table.map_pages(pages, EntryFlags::WRITABLE, frame_allocator_ref.lock().deref_mut())?;

            // add page to the allocator
            zone_allocator.refill(layout, mapping, 0)?; 

            // update the end address of the heap
            heap_end_addr = heap_end_addr.add(ObjectPage8k::SIZE);
            // trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab])
        }
    }

    // store the newly created allocator in the global allocator
    *ALLOCATOR.initial_allocator.lock() = zone_allocator;
    Ok(())
}


/// Returns the initial allocator, the system wide single heap. 
/// The initial allocator lock is held when merging it into a new heap that will be
/// set as the default allocator.
pub fn initial_allocator() -> &'static MutexIrqSafe<ZoneAllocator<'static>>{
    &ALLOCATOR.initial_allocator
}


/// Sets the default allocator for the global heap. It will start being used after this function is called.
/// 
/// # Warning
/// Only call this once the pages already in use by the heap have been added to this new allocator,
/// otherwise there will be deallocation errors.
pub fn set_allocator(allocator: Box<dyn GlobalAlloc + Send + Sync>) {
    ALLOCATOR.set_allocator(allocator);
}


/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, the `initial allocator`. 
/// When a more complex heap is created it is set as the default allocator by initializing the `allocator` field.
pub struct Heap {
    initial_allocator: MutexIrqSafe<ZoneAllocator<'static>>, 
    allocator: Once<Box<dyn GlobalAlloc + Send + Sync>>
}


impl Heap {
    /// Returns a heap in which only the empty initial allocator has been created
    pub const fn empty() -> Heap {
        Heap{
            initial_allocator: MutexIrqSafe::new(ZoneAllocator::new()),
            allocator: Once::new()
        }
    }

    fn set_allocator(&self, allocator: Box<dyn GlobalAlloc + Send + Sync>) {
        self.allocator.call_once(|| allocator);
    }
}

unsafe impl GlobalAlloc for Heap {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // allocate a large object by directly obtaining mapped pages from the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return allocate_large_object(layout).ok().map_or(ptr::null_mut(), |ptr| ptr.as_ptr());
        }

        let res = match self.allocator.try() {
            // use the multiple heaps allocator
            Some(allocator) => {
                allocator.alloc(layout)
            }
            // use the initial allocator
            None => {
                self.initial_allocator.lock().allocate(layout).ok().map_or(ptr::null_mut(), |ptr| ptr.as_ptr()) 
            }
        };

        res
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // deallocate a large object by directly returning mapped pages to the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return deallocate_large_object(ptr, layout);
        }

        match self.allocator.try() {
            // use the multiple heaps allocator            
            Some(allocator) => {

                allocator.dealloc(ptr, layout)
            }
            // use the initial allocator
            None => {
                self.initial_allocator.lock().deallocate(NonNull::new_unchecked(ptr), layout).expect("Deallocation failed!");
            }
        }
    }

}

/// Any memory request greater than MAX_ALLOC_SIZE is satisfied through a request to the OS.
/// The pointer to the beginning of the newly allocated pages is returned.
/// The MappedPages object returned by that request is written to the end of the memory allocated.
/// 
/// # Warning
/// This function should only be used by an allocator in conjunction with [`deallocate_large_object()`](fn.deallocate_large_object.html)
fn allocate_large_object(layout: Layout) -> Result<NonNull<u8>, &'static str> {
    // the mapped pages must have additional memory on the end where we can store the mapped pages object
    let allocation_size = layout.size() + core::mem::size_of::<MappedPages>();

    match create_mapping(allocation_size, HEAP_FLAGS) {
        Ok(mapping) => {
            let ptr = mapping.start_address().value() as *mut u8;

            // This is safe since we ensure that the memory allocated includes space for the MappedPages object,
            // and since the corresponding deallocate function makes sure to retrieve the MappedPages object and drop it.
            unsafe{ (ptr.offset(layout.size() as isize) as *mut MappedPages).write(mapping); }
            // trace!("Allocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);

            NonNull::new(ptr).ok_or("Could not create a non null ptr")
        }
        Err(_e) => {
            // error!("Could not create mapping for a large object in the heap: {:?}", e);
            Err("Could not create mapping for a large object in the heap")
        }
    }
    
}

/// Any memory request greater than MAX_ALLOC_SIZE was created by requesting a MappedPages object from the OS,
/// and now the MappedPages object will be retrieved and dropped to deallocate the memory referenced by `ptr`.
/// 
/// # Warning
/// This function should only be used by an allocator in conjunction with [`allocate_large_object()`](fn.allocate_large_object.html) 
unsafe fn deallocate_large_object(ptr: *mut u8, layout: Layout) {
    // retrieve the mapped pages and drop them
    // This is safe since we ensure that the MappedPages object is read from the offset where it was written
    // by the corresponding allocate function, and the allocate function allocated extra memory for this object in addition to the layout size.
    let _mp = core::ptr::read(ptr.offset(layout.size() as isize) as *const MappedPages); 

    // trace!("Deallocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
}