//! The global allocator for the system. 
//! It starts off as a single fixed size allocator, and when a more complex heap is set up
//! it is set as the default allocator.

#![feature(const_fn)]
#![feature(allocator_api)]
#![feature(const_in_array_repeat_expressions)]
#![no_std]

extern crate alloc;
extern crate linked_list_allocator;
extern crate irq_safety; 
extern crate spin;
extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;

use core::ptr::{self, NonNull};
use alloc::alloc::{GlobalAlloc, Layout};
use memory::{EntryFlags, VirtualAddress, PageTable, AreaFrameAllocator, PageRange, create_mapping, MappedPages};
use kernel_config::memory::PAGE_SIZE;
use irq_safety::MutexIrqSafe;
use core::ops::Add;
use spin::Once;
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use core::ops::DerefMut;

#[global_allocator]
static ALLOCATOR: Heap = Heap::empty();

/// The heap mapped pages should be writable
pub const HEAP_FLAGS: EntryFlags = EntryFlags::WRITABLE;


/// Initializes the initial allocator, which is the first heap used by the system.
pub fn init_initial_allocator(allocator_mutex: &MutexIrqSafe<AreaFrameAllocator>, page_table: &mut PageTable, start_virt_addr: usize, size_in_pages: usize) -> Result<(), &'static str> {
 
    let mapped_pages_per_size_class =  size_in_pages / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * (ObjectPage8k::SIZE/ PAGE_SIZE));
    let mut heap_end_addr = VirtualAddress::new(start_virt_addr)?;
    let mut allocator = allocator_mutex.lock();
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
            let mapping = page_table.map_pages(pages, EntryFlags::WRITABLE, allocator.deref_mut())?;

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

pub fn global_allocator() -> &'static Heap{
    &ALLOCATOR
}

pub fn initial_allocator() -> &'static MutexIrqSafe<ZoneAllocator<'static>>{
    &ALLOCATOR.initial_allocator
}

/// The set of functions that need to be set to switch over to a new allocator.
pub struct GlobalAllocFunctions {
    pub alloc: unsafe fn(Layout) -> *mut u8,
    pub dealloc: unsafe fn(*mut u8, Layout),
}

/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, and when 
/// a more complex heap is initialized it is set as the default allocator.
pub struct Heap {
    initial_allocator: MutexIrqSafe<ZoneAllocator<'static>>, 
    allocator_functions: Once<GlobalAllocFunctions>
}


impl Heap {
    pub const fn empty() -> Heap {
        Heap{
            initial_allocator: MutexIrqSafe::new(ZoneAllocator::new()),
            allocator_functions: Once::new()
        }
    }

    pub fn set_allocator_functions(&self, functions: GlobalAllocFunctions) {
        self.allocator_functions.call_once(|| functions);
    }
}

unsafe impl GlobalAlloc for Heap {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return allocate_large_object(layout).ok().map_or(ptr::null_mut(), |ptr| ptr.as_ptr());
        }
        match self.allocator_functions.try() {
            Some(allocator) => {
                (allocator.alloc)(layout)
            }
            None => {
                match self.initial_allocator.lock().allocate(layout) {
                    Ok(ptr) => ptr.as_ptr(),
                    Err(_) => ptr::null_mut(),
                }
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return deallocate_large_object(ptr, layout);
        }
        match self.allocator_functions.try() {
            Some(allocator) => {
                (allocator.dealloc)(ptr, layout)
            }
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                self.initial_allocator.lock().deallocate(ptr, layout).expect("Deallocation failed!");
            }
        }
    }

}

/// Any memory request greater than MAX_ALLOC_SIZE is satisfied through a request to the OS.
/// The pointer to the beginning of the newly allocated pages is returned.
/// The MappedPages object returned by that request is written to the end of the memory allocated.
pub fn allocate_large_object(layout: Layout) -> Result<NonNull<u8>, &'static str> {
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
        Err(e) => {
            // error!("Could not create mapping for a large object in the heap: {:?}", e);
            Err("Could not create mapping for a large object in the heap")
        }
    }
    
}

/// Any memory request greater than MAX_ALLOC_SIZE was created by requesting a MappedPages object from the OS,
/// and now the MappedPages object will be retrieved and dropped to deallocate the memory referenced by `ptr`.
pub fn deallocate_large_object(ptr: *mut u8, layout: Layout) {
    // retrieve the mapped pages and drop them
    // This is safe since we ensure that the MappedPages object is read from the offset where it was written
    // by the corresponding allocate function, and the allocate function allocated extra memory for this object in addition to the layout size.
    unsafe{ let _mp = core::ptr::read(ptr.offset(layout.size() as isize) as *const MappedPages);}

    // trace!("Deallocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
}