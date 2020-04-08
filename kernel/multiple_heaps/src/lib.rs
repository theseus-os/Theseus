//! An implementation of an allocator that uses multiple heaps. The heap that will be used on each allocation is determined by a key.
//! Right now we use the apic id as the key, so that we have per-core heaps.
//! 
//! The heaps are ZoneAllocators (given in the slabmalloc crate). Each ZoneAllocator maintains 11 separate "slab allocators" for sizes
//! 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 and 8056 (8192 bytes 136 bytes of metadata) bytes.
//! The slab allocator maintains linked lists of allocable pages from which it allocates objects of the same size. 
//! The allocable pages are 8 KiB, and have metadata stored in the last 136 bytes.
//! 
//! In addition to the alloc and dealloc functions, this allocator decides:
//!  * If the requested size is large enough to allocate pages directly from the OS
//!  * If the requested size is small, which heap to actually allocate/deallocate from
//!  * How to deal with OOM errors returned by a heap
//! 
//! Any memory request greater than 8056 bytes is satisfied through a request for pages from the kernel.
//! All other requests are satisfied through the per-core heaps.
//! 
//! The per-core heap which will be used on allocation is determined by the cpu that the task is running on.
//! On deallocation of a block, the heap id is retrieved from metadata at the end of the allocable page which contains the block.
//! 
//! When a per-core heap runs out of memory, pages are first moved between the slab allocators of the per-core heap, then requested from other per-core heaps.
//! If no empty pages are available within any of the per-core heaps, then more memory is allocated from the kernel's heap area.
//!  
//! The maximum number of heaps is configured in the kernel configuration variable, MAX_HEAPS.

#![feature(const_fn)]
#![feature(allocator_api)]
#![feature(const_in_array_repeat_expressions)]
#![no_std]

extern crate alloc;
extern crate linked_list_allocator;
extern crate irq_safety; 
extern crate spin;
#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;
extern crate apic;
extern crate hashbrown;
#[macro_use] extern crate lazy_static;
extern crate heap;

use core::ptr::NonNull;
use alloc::alloc::{GlobalAlloc, Layout};
use memory::{MappedPages, VirtualAddress, FRAME_ALLOCATOR, get_kernel_mmi_ref, PageRange};
use kernel_config::memory::{MAX_HEAPS, PAGE_SIZE, PER_CORE_HEAP_INITIAL_SIZE_PAGES, KERNEL_HEAP_INITIAL_SIZE_PAGES, KERNEL_HEAP_START, KERNEL_HEAP_MAX_SIZE};
use irq_safety::{MutexIrqSafe, RwLockIrqSafe};
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use core::ops::Add;
use core::ops::DerefMut;
use core::ptr;
use hashbrown::HashMap;
use heap::{global_allocator, initial_allocator, GlobalAllocFunctions, allocate_large_object, deallocate_large_object, HEAP_FLAGS};

lazy_static!{ 
    static ref MULTIPLE_HEAPS_ALLOCATOR: MultipleHeaps = MultipleHeaps::empty();
}

/// The size of each MappedPages Object that is allocated for the per-core heaps, in bytes.
/// We curently work with 8KiB, so that the per core heaps can allocate objects up to 8056 bytes.  
const HEAP_MAPPED_PAGES_SIZE_IN_BYTES: usize = ObjectPage8k::SIZE;

/// The size of each MappedPages Object that is allocated for the per-core heaps, in pages.
/// We curently work with 2 pages, so that the per core heaps can allocate objects up to 8056 bytes.   
const HEAP_MAPPED_PAGES_SIZE_IN_PAGES: usize = ObjectPage8k::SIZE / PAGE_SIZE;

/// When an OOM error occurs, before allocating more memory from the OS, we first try to see if there are unused(empty) pages 
/// within the per-core heaps that can be moved to other heaps. To prevent any heap from completely running out of memory we 
/// set this threshold value. A heap must have greater than this number of empty mapped pages to return one for use by other heaps.
const EMPTY_PAGES_THRESHOLD: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES * 2;


/// Initializes the multiple heaps using the apic id as the key, which is mapped to a heap.
/// If we want to change the value the heap id is based on, we would substitute 
/// the lapic iterator with an iterator containing the desired keys.
pub fn initialize_multiple_heaps() -> Result<(), &'static str> {
    for (apic_id, _lapic) in apic::get_lapics().iter() {
        init_individual_heap(*apic_id as usize)?;
    }

    Ok(())       
}


unsafe fn multiple_heaps_allocate(layout: Layout) -> *mut u8 {
    MULTIPLE_HEAPS_ALLOCATOR.alloc(layout) 
}


unsafe fn multiple_heaps_deallocate(ptr: *mut u8, layout: Layout) {
    MULTIPLE_HEAPS_ALLOCATOR.dealloc(ptr, layout)
}


/// Transfers mapped pages belonging to the initial allocator to the first multiple heap
/// and sets the multiple heaps as the default allocator.
/// Only call this function when the multiple heaps are ready to be used.
pub fn switch_to_multiple_heaps() -> Result<(), &'static str> {
    // lock the allocator so that no allocation or deallocation can take place
    let mut initial_allocator = initial_allocator().lock();

    // switch out the initial allocator with an empty heap
    let mut zone_allocator = ZoneAllocator::new();
    core::mem::swap(&mut *initial_allocator, &mut zone_allocator);

    // transfer initial heap to the first multiple heap
    merge_initial_heap(zone_allocator)?;

    let functions = GlobalAllocFunctions {
        alloc: multiple_heaps_allocate,
        dealloc: multiple_heaps_deallocate
    };

    //set the multiple heaps as the default allocator
    global_allocator().set_allocator_functions(functions);

    Ok(())
}


/// Merges the initial allocator into the multiple heap with the smallest heap id
pub fn merge_initial_heap(mut initial_allocator: ZoneAllocator<'static>) -> Result<(), &'static str> {
    let heap_id = MULTIPLE_HEAPS_ALLOCATOR.heaps.read().keys().min().ok_or("Could not find minimum heap id")?.clone();
    MULTIPLE_HEAPS_ALLOCATOR.heaps.write().get(&heap_id).as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Alloc: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .merge(&mut initial_allocator, heap_id)?;
    
    trace!("Merged initial heap into heap {}", heap_id);
    Ok(())
}


/// Allocates pages from the given starting address and maps them to frames.
/// Returns the new mapped pages or an error is returned if the heap memory limit is reached.
fn create_heap_mapping(starting_address: VirtualAddress, size_in_bytes: usize) -> Result<MappedPages, &'static str> {
    if (starting_address.value() + size_in_bytes) >  (KERNEL_HEAP_START + KERNEL_HEAP_MAX_SIZE) {
        return Err("Heap memory limit has been reached");
    }

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_heap_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let mut frame_allocator = FRAME_ALLOCATOR.try()
        .ok_or("create_heap_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();

    let pages = PageRange::from_virt_addr(starting_address, size_in_bytes);
    let heap_flags = HEAP_FLAGS;
    let mp = kernel_mmi.page_table.map_pages(pages, heap_flags, frame_allocator.deref_mut())?;

    // trace!("Allocated heap pages at: {:#X}", starting_address);

    Ok(mp)
}


/// Initializes the heap given by `key`.
/// There are 11 size classes in each heap ranging from [8,16,32..4096,8056 (8192 bytes - 136 bytes metadata)].
/// We evenly distribute the pages allocated for each heap between the size classes. 
pub fn init_individual_heap(key: usize) -> Result<(), &'static str> {
    // check key is within the MAX_HEAPS range
    if key >= MAX_HEAPS {
        warn!("There is a larger key value than the maximum number of heaps in the system");
    }

    let mut heap_end = MULTIPLE_HEAPS_ALLOCATOR.end.lock();
    if heap_end.value() == 0 {
        *heap_end = VirtualAddress::new(KERNEL_HEAP_START + (KERNEL_HEAP_INITIAL_SIZE_PAGES * PAGE_SIZE))?;
    }
    let mut heap_end_addr = *heap_end;

    let mapped_pages_per_size_class = PER_CORE_HEAP_INITIAL_SIZE_PAGES / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * HEAP_MAPPED_PAGES_SIZE_IN_PAGES);
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
            let mapping = create_heap_mapping(heap_end_addr, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;

            // add page to the allocator
            zone_allocator.refill(layout, mapping, key)?; 

            // update the end address of the heap
            heap_end_addr = heap_end_addr.add(HEAP_MAPPED_PAGES_SIZE_IN_BYTES);
            // trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab]);
        }
    }

    // store the new end of the heap after this core has been initialized
    *heap_end = heap_end_addr;

    // store the newly created allocator in the global allocator
    if let Some(_heap) = MULTIPLE_HEAPS_ALLOCATOR.heaps.write().insert(key, MutexIrqSafe::new(zone_allocator)) {
        return Err("New heap created with a previously used id");
    }

    trace!("Created heap {} with max alloc size: {} bytes", key, ZoneAllocator::MAX_ALLOC_SIZE);

    Ok(())
}


/// Returns the key which determines the heap that will be used.
/// Currently we use the apic id as the key, but we can replace it with some
/// other value e.g. task id
fn get_key() -> usize {
    apic::get_my_apic_id()
        .ok_or("Heap:: Could not retrieve apic id")
        .expect("Heap:: Could not retrieve apic id") as usize
}


/// An allocator that contains multiple heaps. The heap that is used on each allocation is
/// determined by a key. Currently the apic id is used as the key.
pub struct MultipleHeaps{
    /// the per-core heaps
    heaps: RwLockIrqSafe<HashMap<usize, MutexIrqSafe<ZoneAllocator<'static>>>>,
    /// We currently don't return memory back to the OS. Because of this all memory in the heap is contiguous
    /// and extra memory for the heap is always allocated from the end.
    /// The Mutex also serves the purpose of helping to synchronize new allocations.
    end: MutexIrqSafe<VirtualAddress>, 
}

impl MultipleHeaps {
    pub fn empty() -> MultipleHeaps {
        MultipleHeaps{
            heaps: RwLockIrqSafe::new(HashMap::new()),
            end: MutexIrqSafe::new(VirtualAddress::zero())
        }
    }


    unsafe fn allocate_from_heap(&self, heap_id: usize, layout: Layout) -> Result<NonNull<u8>, &'static str> {
        self.heaps.read().get(&heap_id).as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Alloc: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .allocate(layout)
    }


    unsafe fn deallocate_from_heap(&self, heap_id: usize, ptr: *mut u8, layout: Layout) -> Result<(), &'static str> {
        self.heaps.read().get(&heap_id).as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Dealloc: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .deallocate(NonNull::new_unchecked(ptr), layout)
    }

    /// Add pages to the heap.
    ///     
    /// # Arguments
    /// * `layout`: layout.size will determine which allocation size the page will be used for. 
    /// * `mp`: MappedPages object representing the pages being added to the heap.
    /// * `heap_id`: heap the page is being added to.
    fn refill_heap(&self, layout: Layout, mp: MappedPages, heap_id: usize) -> Result<(), &'static str> {
        self.heaps.read().get(&heap_id).as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Refill: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .refill(layout, mp, heap_id)
    }


    /// Retrieves an empty page from the heap which has the maximum number of empty pages,
    /// if the maximum is greater than a threshold.
    fn retrieve_empty_page(&self) -> Result<MappedPages, &'static str> {
        let heaps = self.heaps.read();
        let heap = heaps.values().max_by_key(
            |&heap| heap.lock().empty_pages())
            .ok_or("Per core heaps haven't been initialized")?;
        
        if heap.lock().empty_pages() > EMPTY_PAGES_THRESHOLD {
            return heap.lock().retrieve_empty_page().ok_or("No empty page in the heaps");
        }

        Err("No empty pages available")
    }


    /// Called when an call to allocate() returns a null pointer. The following steps are used to recover memory:
    /// (1) Pages are exchanged between per-core heaps.
    /// (2) If the above fails, then more pages are allocated from the OS.
    /// 
    /// An Err is returned if there is no more memory to be allocated in the heap memory area.
    /// 
    /// # Arguments
    /// * `layout`: layout.size will determine which allocation size the retrieved pages will be used for. 
    /// * `heap_id`: heap that needs to grow.
    fn grow_heap(&self, layout: Layout, heap_id: usize) -> Result<(), &'static str> {
        // (1) Try to retrieve a page from another heap
        let mp = 
            match self.retrieve_empty_page() {
                Ok(mp) => {
                    trace!("grow_heap:: retrieved a page from another heap to refill core heap {} for size :{}", heap_id, layout.size());
                    mp   
                }
                // (2) If that didn't work ry to allocate memory from the OS
                Err(_e) => {
                    let mut heap_end = self.end.lock();
                    let mp = create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                    trace!("grow_heap:: Allocated a page to refill core heap {} for size :{} at address: {:#X}", heap_id, layout.size(), *heap_end);
                    *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                    mp
                }
            };
        self.refill_heap(layout, mp, heap_id)
    }

}

unsafe impl GlobalAlloc for MultipleHeaps {

    /// Allocates the given `layout` from the heap of the core the task is currently running on.
    /// If the size requested is greater than MAX_ALLOC_SIZE, then memory is directly requested from the OS.
    /// If the per-core heap is not initialized, then an error is returned.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let id = get_key();

        let alloc_result = 
            // allocate a large object directly through mapped pages
            if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
                allocate_large_object(layout)
            }
            // allocate an object with the per-core heap if initialized 
            else if self.heaps.read().get(&id).is_some() {
                match self.allocate_from_heap(id, layout) {
                    Ok(ptr) => Ok(ptr),
                    // If a null pointer was returned, then there are no available empty pages in the heap
                    Err(_e) => {
                        self.grow_heap(layout, id).and_then(|_res| self.allocate_from_heap(id,layout))
                        // self.allocate_from_heap(id, layout)
                    }
                }
            }
            else {
                Err("MultipleHeaps: Heap was not initialized")
            };
        
        alloc_result                
            .ok()
            .map_or(ptr::null_mut(), |allocation| allocation.as_ptr())    
    }

    /// Deallocates the memory at the address given by `ptr`.
    /// If the size being returned is greater than MAX_ALLOC_SIZE, then memory is directly returned to the OS.
    /// Otherwise, it is returned to the per-core heap it was allocated from.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // deallocate a large object directly through mapped pages
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            deallocate_large_object(ptr,layout)
        }

        // deallocate an object with the per-core heap it was allocated from 
        else {
            // find the starting address of the object page this block belongs to
            let page_addr = (ptr as usize) & !(ObjectPage8k::SIZE - 1);
            // find the heap id
            let id = *((page_addr as *mut u8).offset(ObjectPage8k::HEAP_ID_OFFSET as isize) as *mut usize);
            self.deallocate_from_heap(id, ptr, layout).expect("Couldn't deallocate");
        }
    }
}


