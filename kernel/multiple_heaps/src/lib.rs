//! An implementation of an allocator that uses per-core heaps
//! 
//! The allocator is made up of multiple heaps. It is first initialized as a single linked-list allocator to take care of early memory requests.
//! Once all cores have been discovered, the per-core heaps are initialized and are the only allocators used after that.
//! The per-core heaps are ZoneAllocators (given in the slabmalloc crate). Each ZoneAllocator maintains 11 separate "slab allocators" for sizes
//! 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 and 8040 (8192 bytes - 152 bytes of metadata) bytes.
//! The slab allocator maintains linked lists of allocable pages from which it allocates objects of the same size. 
//! The allocable pages are 8 KiB, and have metadata stored in the last 152 bytes.
//! 
//! In addition to the alloc and dealloc functions, this allocator decides:
//!  * If the requested size is large enough to allocate pages directly from the OS
//!  * If the requested size is small, which heap to actually allocate/deallocate from
//!  * How to deal with OOM errors returned by a heap
//! 
//! Any memory request greater than 8040 bytes is satisfied through a request for pages from the kernel.
//! All other requests are satified through the per-core heaps.
//! 
//! The per-core heap which will be used on allocation is determined by the cpu that the task is running on.
//! On deallocation of a block, the heap id is retrieved from metadata at the end of the allocable page which contains the block.
//! 
//! When a per-core heap runs out of memory, pages are first moved between the slab allocators of the per-core heap, then requested from other per-core heaps.
//! If no empty pages are availabe within any of the per-core heaps, then more memory is allocated from the kernel's heap area.
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
extern crate raw_cpuid;
#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;

use core::ptr::NonNull;
use alloc::alloc::{GlobalAlloc, Layout};
use raw_cpuid::CpuId;
use memory::{MappedPages, create_mapping, EntryFlags, create_heap_mapping, VirtualAddress};
use kernel_config::memory::{MAX_HEAPS, PAGE_SIZE, PER_CORE_HEAP_INITIAL_SIZE_PAGES};
use irq_safety::{MutexIrqSafe, RwLockIrqSafe};
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use core::ops::Add;

#[global_allocator]
static ALLOCATOR: MultipleHeaps = MultipleHeaps::empty();

/// The size of each MappedPages Object that is allocated for the per-core heaps, in bytes.
/// We curently work with 8KiB, so that the per core heaps can allocate objects up to 8040 bytes.  
pub const HEAP_MAPPED_PAGES_SIZE_IN_BYTES: usize = ObjectPage8k::SIZE;

/// The size of each MappedPages Object that is allocated for the per-core heaps, in pages.
/// We curently work with 2 pages, so that the per core heaps can allocate objects up to 8040 bytes.   
pub const HEAP_MAPPED_PAGES_SIZE_IN_PAGES: usize = ObjectPage8k::SIZE / PAGE_SIZE;

pub const HEAP_FLAGS: EntryFlags = EntryFlags::WRITABLE;


/// When an OOM error occurs, before allocating more memory from the OS, we first try to see if there are unused(empty) pages 
/// within the per-core heaps that can be moved to other heaps. To prevent any heap from completely running out of memory we 
/// set this threshold value. A heap must have greater than this number of empty mapped pages to return one for use by other heaps.
pub const EMPTY_PAGES_THRESHOLD: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES * 2;

/// Initializes the initial allocator, which is the first heap used by the system.
/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
pub fn init_initial_allocator(start_virt_addr: usize, size_in_bytes: usize) -> Result<(), &'static str>{
    let mut heap_end = ALLOCATOR.end.lock();
    unsafe { ALLOCATOR.initial_allocator.lock().init(start_virt_addr, size_in_bytes); }
    *heap_end = VirtualAddress::new(ALLOCATOR.initial_allocator.lock().top())?;
    Ok(())
}

/// Initializes the per core heap given by `core_id`.
/// There are 11 size classes in each per-core heap ranging from [8,16,32..4096,8040 (8192 bytes - 152 bytes metadata)].
/// We evenly distribute the pages allocated for each per-core heap between the size classes. 
pub fn init_per_core_heap(core_id: usize) -> Result<(), &'static str> {
    // check core id is within the MAX_HEAPS range
    if core_id >= MAX_HEAPS {
        error!("Not a valid core id for a heap");
        return Err("Invalid core id for a heap");
    }

    let mapped_pages_per_size_class =  PER_CORE_HEAP_INITIAL_SIZE_PAGES / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * HEAP_MAPPED_PAGES_SIZE_IN_PAGES);

    // locking the end variable of the allocator ensures that only one task is allocating heap memory at a time
    let mut heap_end = ALLOCATOR.end.lock();
    let mut heap_end_addr = *heap_end;

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
            zone_allocator.refill(layout, mapping, core_id)?; 

            // update the end address of the heap
            heap_end_addr = heap_end_addr.add(HEAP_MAPPED_PAGES_SIZE_IN_BYTES);
            // trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab]);
        }
    }
    
    // store the new end of the heap after this core has been initialized
    *heap_end = heap_end_addr;

    // store the newly created allocator in the global allocator
    ALLOCATOR.heaps.write()[core_id] = Some(MutexIrqSafe::new(zone_allocator));

    trace!("Created per core heap {} with max alloc size: {} bytes", core_id, ZoneAllocator::MAX_ALLOC_SIZE);

    Ok(())
}

/// An allocator that contains per-core heaps and a linked-list allocator that
/// is used initially before the per-core heaps are set up.
pub struct MultipleHeaps{
    /// the per-core heaps
    heaps: RwLockIrqSafe<[Option<MutexIrqSafe<ZoneAllocator<'static>>>; MAX_HEAPS]>,
    /// the first heap that is initialized and is only used until the per-core heaps are initialized
    initial_allocator: MutexIrqSafe<linked_list_allocator::Heap>,
    /// We currently don't return memory back to the OS. Because of this all memory in the heap is contiguous
    /// and extra memory for the heap is always allocated from the end.
    /// The Mutex also serves the purpose of helping to synchronize new allocations.
    end: MutexIrqSafe<VirtualAddress>, 
}

impl MultipleHeaps {
    pub const fn empty() -> MultipleHeaps {
        MultipleHeaps{
            heaps: RwLockIrqSafe::new([None; MAX_HEAPS]),
            initial_allocator: MutexIrqSafe::new(linked_list_allocator::Heap::empty()),
            end: MutexIrqSafe::new(VirtualAddress::zero())
        }
    }

    /// Returns true if 1 or more per-core heaps have been initialized
    fn per_core_heaps_initialized(&self) -> bool {
        for heap in self.heaps.read().iter() {
            if heap.is_some() {
                return true;
            }
        }
        false
    }

    /// The starting and end address of memory assigned to the initial allocator
    fn initial_allocator_range(&self) -> core::ops::Range<usize> {
        let initial_allocator = self.initial_allocator.lock();
        (initial_allocator.bottom()..initial_allocator.top())
    }


    unsafe fn allocate_from_core_heap(&self, heap_id: usize, layout: Layout) -> Result<NonNull<u8>, &'static str> {
        self.heaps.read()[heap_id].as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Alloc: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .allocate(layout)
    }


    unsafe fn deallocate_from_core_heap(&self, heap_id: usize, ptr: *mut u8, layout: Layout) -> Result<(), &'static str> {
        self.heaps.read()[heap_id].as_ref()
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
    fn refill_core_heap(&self, layout: Layout, mp: MappedPages, heap_id: usize) -> Result<(), &'static str> {
        self.heaps.read()[heap_id].as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Refill: per core heap was not initialized") // we want a panic here since that means something was wrong in the initialization steps
            .lock()
            .refill(layout, mp, heap_id)
    }

    /// Scans a per-core heap's allocators to see if any has a empty pages that can be retrieved, and gives them to another of the heap's allocators.
    /// Returns an Err if no empty page is available.
    /// 
    /// # Arguments
    /// * `layout`: layout.size will determine which allocation size the page will be used for. 
    /// * `heap_id`: heap the page is being moved within.
    fn exchange_pages_within_core_heap(&self, layout: Layout, heap_id: usize) -> Result<(), &'static str> {
        self.heaps.read()[heap_id].as_ref()
            .ok_or("Core heap is not initialized!")
            .expect("Exchange pages: per core heap was not initialized")
            .lock()
            .exchange_pages_within_heap(layout, heap_id) 
    }

    /// Retrieves an empty page from the heap which has the maximum number of empty pages,
    /// if the maximum is greater than a threshold.
    fn retrieve_empty_page(&self) -> Result<MappedPages, &'static str> {
        if self.per_core_heaps_initialized() {
            let id = self.heaps.read().iter().enumerate().max_by_key(
                |&(_i, val)| val.as_ref().map_or(0, |heap| heap.lock().empty_pages()))
            .map(|(i, _val)| i).ok_or("There was no per core heap even though they have been initialized")?;

            if let Some(heap) = &self.heaps.read()[id] {
                if heap.lock().empty_pages() > EMPTY_PAGES_THRESHOLD {
                    return heap.lock().retrieve_empty_page().ok_or("No empty page in the heaps");
                }
            }
        }

        Err("Per core heaps haven't been initialized")
    }

    /// Called when an call to allocate() returns a null pointer. The following steps are used to recover memory:
    /// (1) Pages are exchanged within a per-core heap.
    /// (2) Pages are exchanged between per-core heaps.
    /// (3) If the above two fail, then more pages are allocated from the OS.
    /// 
    /// An Err is returned if there is no more memory to be allocated in the heap memory area.
    /// 
    /// # Arguments
    /// * `layout`: layout.size will determine which allocation size the retrieved pages will be used for. 
    /// * `heap_id`: heap that needs to grow.
    fn grow_heap(&self, layout: Layout, heap_id: usize) -> Result<(), &'static str> {
        // (1) Try to exchange pages within a heap
        match self.exchange_pages_within_core_heap(layout, heap_id) {
            Ok(()) => {
                trace!("grow_heap:: exchanged pages within a core heap {} for size :{}", heap_id, layout.size());
                Ok(())
            }

            // (2) If that didn't work, try to retrieve a page from another heap
            Err(_e) => {
                let mp = 
                    match self.retrieve_empty_page() {
                        Ok(mp) => {
                            trace!("grow_heap:: retrieved a page from another heap to refill core heap {} for size :{}", heap_id, layout.size());
                            mp   
                        }
                        // (3) Lastly, try to allocate memory from the OS
                        Err(_e) => {
                            let mut heap_end = self.end.lock();
                            let mp = create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                            trace!("grow_heap:: Allocated a page to refill core heap {} for size :{} at address: {:#X}", heap_id, layout.size(), *heap_end);
                            *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                            mp
                        }
                    };
                self.refill_core_heap(layout, mp, heap_id)
            }   
        }
    }

}

unsafe impl GlobalAlloc for MultipleHeaps {

    /// Allocates the given `layout` from the heap of the core the task is currently running on.
    /// If the size requested is greater than MAX_ALLOC_SIZE, then memory is directly requested from the OS.
    /// If the per-core heap is not initialized, then the initial allocator is used.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let id = CpuId::new().get_feature_info()
            .expect("Could not retrieve cpuid")
            .initial_local_apic_id() as usize % MAX_HEAPS; // in systems where the number of cores is greater than the given maximum, heaps will have to be shared by cores.

        let alloc_result = 
            // allocate a large object directly through mapped pages
            if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
                allocate_large_object(layout)
            }
            // allocate an object with the per-core heap if initialized 
            else if self.heaps.read()[id].is_some() {
                match self.allocate_from_core_heap(id, layout) {
                    Ok(ptr) => Ok(ptr),
                    Err(_e) => {
                        let _ = self.grow_heap(layout, id);
                        self.allocate_from_core_heap(id, layout)
                    }
                }
            }
            // allocate with the initial allocator
            // we should never have to allocate from here once the per-core heaps are initialized
            else {
                self.initial_allocator
                    .lock()
                    .allocate_first_fit(layout)
                    .map_err(|_e| "Fallback allocator could not allocate the given layout")
            };
        
        alloc_result                
            .ok()
            .map_or(0 as *mut u8, |allocation| allocation.as_ptr())    
    }

    /// Deallocates the memory at the address given by `ptr`.
    /// If the size being returned is greater than MAX_ALLOC_SIZE, then memory is directly returned to the OS.
    /// If the `ptr` lies within the initial allocator's memory range then memory is returned to the initial allocator.
    /// Otherwise, it is returned to the per-core heap it was allocated from.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // deallocate a large object directly through mapped pages
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            deallocate_large_object(ptr,layout)
        }

        // Check if the pointer falls in the memory region we gave to the initial allocator.
        // We have to check for this first since even though no new allocations may be made with the initial allocator 
        // once the per-core heaps are initialized, deallocations might still occur.
        else if self.initial_allocator_range().contains(&(ptr as usize)) {
            self.initial_allocator
                .lock()
                .deallocate(NonNull::new_unchecked(ptr), layout)
        }

        // deallocate an object with the per-core heap it was allocated from 
        else {
            // find the starting address of the object page this block belongs to
            let page_addr = (ptr as usize) & !(ObjectPage8k::SIZE - 1);
            // find the heap id
            let id = *((page_addr as *mut u8).offset(ObjectPage8k::HEAP_ID_OFFSET as isize) as *mut usize);
            self.deallocate_from_core_heap(id, ptr, layout).expect("Couldn't deallocate");
        }
    }
}


/// Any memory request greater than MAX_ALLOC_SIZE is satisfied through a request to the OS.
/// The pointer to the beginning of the newly allocated pages is returned.
/// The MappedPages object returned by that request is written to the end of the memory allocated.
/// 
/// This is safe since we ensure that the memory allocated includes space for the MappedPages object,
/// and since the corresponding deallocate function makes sure to retrieve the MappedPages object and drop it. 
unsafe fn allocate_large_object(layout: Layout) -> Result<NonNull<u8>, &'static str> {
    // the mapped pages must have additional memory on the end where we can store the mapped pages object
    let allocation_size = layout.size() + core::mem::size_of::<MappedPages>();

    match create_mapping(allocation_size, HEAP_FLAGS) {
        Ok(mapping) => {
            let ptr = mapping.start_address().value() as *mut u8;
            (ptr.offset(layout.size() as isize) as *mut MappedPages).write(mapping);
            // trace!("Allocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);

            NonNull::new(ptr).ok_or("Could not create a non null ptr")
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
/// This is safe since we ensure that the MappedPages object is read from the offset where it was written
/// by the corresponding allocate function, and the allocate function allocated extra memory for this object in addition to the layout size.
unsafe fn deallocate_large_object(ptr: *mut u8, layout: Layout) {
    //retrieve the mapped pages and drop them
    let _mp = core::ptr::read(ptr.offset(layout.size() as isize) as *const MappedPages); 

    // trace!("Deallocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
}