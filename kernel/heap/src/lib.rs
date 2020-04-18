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
#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;
extern crate hashbrown;

use core::ptr::{self, NonNull};
use alloc::alloc::{GlobalAlloc, Layout};
use memory::{EntryFlags, VirtualAddress, PageTable, PageRange, create_mapping, MappedPages, FrameAllocator, FrameAllocatorRef};
use kernel_config::memory::PAGE_SIZE;
use irq_safety::MutexIrqSafe;
use core::ops::Add;
use spin::Once;
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage, Allocator};
use core::ops::DerefMut;
use alloc::boxed::Box;
use hashbrown::HashMap;
use core::sync::atomic::{AtomicBool, Ordering};

#[global_allocator]
static ALLOCATOR: Heap = Heap::empty();

/// The heap mapped pages should be writable
pub const HEAP_FLAGS: EntryFlags = EntryFlags::WRITABLE;

/// The maximum number of large allocations the heap can store at a time. 
/// The inital hashmap created for storing large allocation will have this capacity.
const MAX_LARGE_ALLOCATIONS: usize = 100;

/// The number of pages each size class in the ZoneAllocator in the initial heap is initialized with, for the initial heap.
const PAGES_PER_SIZE_CLASS: usize = 372; 

/// Size of the initial heap. It's approximately 16 MiB.
pub const HEAP_INITIAL_SIZE_PAGES: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES *  PAGES_PER_SIZE_CLASS;

/// Synchronization variable to know when the large allocations hashmap is being resized 
static RESIZING_LARGE_ALLOCATIONS_HASHMAP: AtomicBool = AtomicBool::new(false);


/// Initializes the initial allocator, which is the first heap used by the system.
pub fn init_single_heap<A: FrameAllocator>(
    frame_allocator_ref: &FrameAllocatorRef<A>, page_table: &mut PageTable, start_virt_addr: usize, size_in_pages: usize
) -> Result<(), &'static str> {
 
    let mapped_pages_per_size_class =  size_in_pages / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * (ObjectPage8k::SIZE/ PAGE_SIZE));
    let mut heap_end_addr = VirtualAddress::new(start_virt_addr)?;
    let mut zone_allocator = ZoneAllocator::new(0);

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
            zone_allocator.refill(layout, mapping)?; 

            // update the end address of the heap
            heap_end_addr = heap_end_addr.add(ObjectPage8k::SIZE);
            // trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab])
        }
    }
    // store the newly created allocator in the global allocator
    *ALLOCATOR.initial_allocator.lock() = zone_allocator;
    // this first hashmap will have a small capacity that can be satisfied without creating a new mapping
    // later on we'll expand the capacity when large objects can be allocated.
    *ALLOCATOR.large_allocations.lock() = Some(HashMap::with_capacity(MAX_LARGE_ALLOCATIONS));

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


// /// Increases the capacity of the hashmap storing large allocations.
// pub fn expand_capacity_for_large_objects() -> Result<(), &'static str> {
//     let mut new_map = HashMap::with_capacity(MAX_LARGE_ALLOCATIONS);

//     // acquire the lock so that no large allocations can take place during this process
//     let mut prev_map = ALLOCATOR.large_allocations.lock();
//     let large_allocations = prev_map.as_mut().ok_or("large allocations hashmap was not initialized")?;
//     let allocation_pairs = large_allocations.drain();

//     // Add all the previous mappings into the new hashmap
//     for pair in allocation_pairs {
//         new_map.insert(pair.0, pair.1);
//     }

//     // set the new hashmap as the default
//     *large_allocations = new_map;
//     Ok(())
// }


/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, the `initial allocator`. 
/// When a more complex heap is created it is set as the default allocator by initializing the `allocator` field.
pub struct Heap {
    initial_allocator: MutexIrqSafe<ZoneAllocator<'static>>, 
    allocator: Once<Box<dyn GlobalAlloc + Send + Sync>>,
    large_allocations: MutexIrqSafe<Option<HashMap<usize, MappedPages>>>
}


impl Heap {
    /// Returns a heap in which only the empty initial allocator has been created
    pub const fn empty() -> Heap {
        Heap{
            initial_allocator: MutexIrqSafe::new(ZoneAllocator::new(0)),
            allocator: Once::new(),
            large_allocations: MutexIrqSafe::new(None),
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
            return allocate_large_object(
                layout, 
                &mut self.large_allocations.lock().as_mut().expect("Hashmap for large allocations was not initialized")
            ).map(|allocation| allocation.as_ptr()).unwrap_or(ptr::null_mut())
        }

        let res = match self.allocator.try() {
            Some(allocator) => {
                allocator.alloc(layout)
            }
            None => { // use the initial allocator            
                self.initial_allocator.lock().allocate(layout).map(|allocation| allocation.as_ptr()).unwrap_or(ptr::null_mut()) 
            }
        };

        res
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // deallocate a large object by directly returning mapped pages to the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return deallocate_large_object(
                ptr, 
                layout, 
                &mut self.large_allocations.lock().as_mut().expect("Hashmap for large allocations was not initialized")
            )
        }
            
        match self.allocator.try() {
            Some(allocator) => {
                allocator.dealloc(ptr, layout)
            }
            None => { // use the initial allocator
                self.initial_allocator.lock().deallocate(NonNull::new_unchecked(ptr), layout).expect("Deallocation failed!");
            }
        }
    }

}

/// Any memory request greater than MAX_ALLOC_SIZE is satisfied through a request to the OS.
/// The pointer to the beginning of the newly allocated pages is returned.
/// The MappedPages object returned by that request is stored in a hashmap.
/// 
/// # Warning
/// This function should only be used by an allocator in conjunction with [`deallocate_large_object()`](fn.deallocate_large_object.html)
fn allocate_large_object(layout: Layout, map: &mut HashMap<usize, MappedPages>) -> Result<NonNull<u8>, &'static str> {
    if map.len() >= map.capacity() {
        error!("Exceeded storage capacity for large allocations. The current capacity is {}.
                We still need to improve this by allowing for any number of large allocations, without falling into a loop of allocate_large_object().
                For such a high number of large allocations, applications should be directly allocating MappedPages objects rather than from the heap", map.capacity());
        return Err("Exceeded storage capacity for large allocations");
    }

    match create_mapping(layout.size(), HEAP_FLAGS) {
        Ok(mapping) => {
            let ptr = mapping.start_address().value();
            // trace!("Allocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
            map.insert(ptr as usize, mapping);
            NonNull::new(ptr as *mut u8).ok_or("Could not create a non null ptr")

        }
        Err(e) => {
            error!("Could not create mapping for a large object in the heap: {:?}", e);
            Err("Could not create mapping for a large object in the heap")
        }
    }
    
}

/// Any memory request greater than MAX_ALLOC_SIZE was created by requesting a MappedPages object from the OS,
/// and now the MappedPages object will be retrieved and dropped to deallocate the memory referenced by `ptr`.
/// 
/// # Warning
/// This function should only be used by an allocator in conjunction with [`allocate_large_object()`](fn.allocate_large_object.html) 
fn deallocate_large_object(ptr: *mut u8, _layout: Layout, map: &mut HashMap<usize, MappedPages>) {
    let _mp = map.remove(&(ptr as usize))
        .expect("Invalid ptr was passed to deallocate_large_object. There is no such mapping stored");
    // trace!("Deallocated a large object of {} bytes at address: {:#X} {:#X}", layout.size(), ptr as usize, mp.start_address());
}