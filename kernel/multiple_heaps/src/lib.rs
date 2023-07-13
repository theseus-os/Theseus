//! An implementation of an allocator that uses multiple heaps. The heap that will be used on each allocation is determined by a key.
//! Right now we use the apic id as the key, so that we have per-core heaps.
//! 
//! The heaps are ZoneAllocators (given in the slabmalloc crate). Each ZoneAllocator maintains 11 separate "slab allocators" for sizes
//! 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 and (8KiB - bytes of metadata) bytes. The maximum allocation size is given by `ZoneAllocator::MAX_ALLOC_SIZE`.
//! The slab allocator maintains linked lists of allocable pages from which it allocates objects of the same size. 
//! The allocable pages are 8 KiB (`ObjectPage8k::SIZE`), and have metadata stored in the ending bytes (`ObjectPage8k::METADATA_SIZE`).
//! The metadata includes a heap id, MappedPages object this allocable page belongs to, forward and back pointers to pages stored in a linked list and a
//! bitmap to keep track of allocations. The maximum allocation size can change if the size of the objects in the metadata change. If that happens it will be automatically
//! reflected in the constants `ZoneAllocator::MAX_ALLOC_SIZE` and `ObjectPage8k::METADATA_SIZE`
//! 
//! Any memory request greater than maximum allocation size, a large allocation, is satisfied through a request for pages from the kernel.
//! All other requests are satisfied through the per-core heaps.
//! 
//! The per-core heap which will be used on allocation is determined by the cpu that the task is running on.
//! On deallocation of a block, the heap id is retrieved from metadata at the end of the allocable page which contains the block.
//! 
//! When a per-core heap runs out of memory, pages are first moved between the slab allocators of the per-core heap, then requested from other per-core heaps.
//! If no empty pages are available within any of the per-core heaps, then more virtual pages are allocated from the range of virtual addresses dedicated to the heap
//! [KERNEL_HEAP_START](../kernel_config/memory/constant.KERNEL_HEAP_START.html) and dynamically mapped to physical memory frames.

#![feature(allocator_api)]
#![no_std]

extern crate sync_irq; 
#[macro_use] extern crate log;
extern crate memory;
extern crate page_allocator;
extern crate kernel_config;
extern crate apic;
extern crate heap;
extern crate hashbrown;
#[macro_use] extern crate cfg_if;

#[cfg(all(not(unsafe_heap), not(safe_heap)))]
extern crate slabmalloc;

#[cfg(unsafe_heap)]
extern crate slabmalloc_unsafe;

#[cfg(safe_heap)]
extern crate slabmalloc_safe;

use core::ptr::NonNull;
use alloc::alloc::{GlobalAlloc, Layout};
use alloc::boxed::Box;
use hashbrown::HashMap;
use memory::{MappedPages, VirtualAddress, get_kernel_mmi_ref, create_mapping};
use kernel_config::memory::{PAGE_SIZE, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use core::ops::Deref;
use core::ptr;
use heap::HEAP_FLAGS;
use sync_irq::IrqSafeMutex;
use page_allocator::{DeferredAllocAction, allocate_pages_by_bytes_deferred};

#[cfg(all(not(unsafe_heap), not(safe_heap)))]
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage, MappedPages8k};

#[cfg(unsafe_heap)]
use slabmalloc_unsafe::{ZoneAllocator, ObjectPage8k, AllocablePage};

#[cfg(safe_heap)]
use slabmalloc_safe::{ZoneAllocator, ObjectPage8k, AllocablePage, MappedPages8k};

/// The size in bytes of each "group" or "set" of MappedPages objects that is allocated for each heap's slab.
/// We curently work with 8KiB sets, such that the per-core heaps can allocate objects up to `ZoneAllocator::MAX_ALLOC_SIZE`.  
const HEAP_MAPPED_PAGES_SIZE_IN_BYTES: usize = ObjectPage8k::SIZE;

/// The size in pages of each heap's page set, see `HEAP_MAPPED_PAGES_SIZE_IN_BYTES`.
const HEAP_MAPPED_PAGES_SIZE_IN_PAGES: usize = HEAP_MAPPED_PAGES_SIZE_IN_BYTES / PAGE_SIZE;

/// When an OOM error occurs, before allocating more memory from the OS, we first try to see if there are unused(empty) pages 
/// within the per-core heaps that can be moved to other heaps. To prevent any heap from completely running out of memory we 
/// set this threshold value. A heap must have greater than this number of empty mapped pages to return one for use by other heaps.
const EMPTY_PAGES_THRESHOLD: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES * 2;

/// The number of pages each size class in the ZoneAllocator is initialized with. It is approximately 512 KiB.
const PAGES_PER_SIZE_CLASS: usize = 128; // was 24 

/// Starting size of each per-core heap. 
pub const PER_CORE_HEAP_INITIAL_SIZE_PAGES: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES *  PAGES_PER_SIZE_CLASS;

/// The number of heap page sets that are requested from the OS whenever the heap is grown.
/// This should be minimum `2` in order to ensure that there is at least:
/// * one empty page for the requested allocation, and
/// * one empty page for the deferred allocations to occur. 
/// 
/// # Important Note
/// The total size of heap objects allocated during/by the deferred alloc actions must be able to fit 
/// into the additional page(s) here, which is currently just one extra heap page set (currently 8KiB). 
/// Currently, each `DeferredAllocAction` creates 3 chunks, so that means that the current calculation is:
/// `(3 * HEAP_GROWTH_AMOUNT * sizeof(Chunk)` bytes must fit within one 8KiB heap page set.
const HEAP_GROWTH_AMOUNT: usize = 2;

/// Creates and initializes the multiple heaps using the apic id as the key, which is mapped to a heap.
/// If we want to change the value the heap id is based on, we would substitute 
/// the lapic iterator with an iterator containing the desired keys.
fn initialize_multiple_heaps() -> Result<MultipleHeaps, &'static str> {
    let mut multiple_heaps = MultipleHeaps::empty();

    for (apic_id, _lapic) in apic::get_lapics().iter() {
        init_individual_heap(apic_id.value() as usize, &mut multiple_heaps)?;
    }

    Ok(multiple_heaps)       
}


/// The setup routine for multiple heaps. It creates and initializes the multiple heaps,
/// then sets the multiple heaps as the default allocator.
/// Only call this function when the multiple heaps are ready to be used.
pub fn switch_to_multiple_heaps() -> Result<(), &'static str> {
    let multiple_heaps = Box::new(initialize_multiple_heaps()?);
    //set the multiple heaps as the default allocator
    heap::set_allocator(multiple_heaps);

    Ok(())
}



/// Allocates pages from the given starting address and maps them to frames.
/// Returns the new mapped pages or an error if the heap memory limit is reached.
fn create_heap_mapping(
    starting_address: VirtualAddress, 
    size_in_bytes: usize
) -> Result<(MappedPages, DeferredAllocAction<'static>), &'static str> {
    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_heap_mapping(): KERNEL_MMI was not yet initialized!")?;
    let (pages, action) = allocate_pages_by_bytes_deferred(
        page_allocator::AllocationRequest::AtVirtualAddress(starting_address),
        size_in_bytes,
    ).map_err(|_e| "create_heap_mapping(): failed to allocate pages at the starting address")?;
    if pages.start_address().value() % HEAP_MAPPED_PAGES_SIZE_IN_BYTES != 0 {
        return Err("multiple_heaps: the allocated pages for the heap wasn't properly aligned");
    }
    let mp = kernel_mmi_ref.lock().page_table.map_allocated_pages(pages, HEAP_FLAGS)?;
    // trace!("Allocated heap pages at: {:#X}", starting_address);
    Ok((mp, action))
}


// Initialization function for the heap differs depending on the slabmalloc version used.
//
// For the unsafe version, the new heap mapping is merged into the heap MappedPages object in the kernel mmi
// and then a reference to the starting address is passed to the ZoneAllocator.
//
// For the default and safe versions, MappedPages8k objects are created from the new heap mapping and passed to the ZoneAllocator.
cfg_if! {
if #[cfg(unsafe_heap)] {
    extern crate alloc;
    extern crate spin;

    use spin::Once;

    /// Initializes the heap given by `key`.
    /// There are 11 size classes in each heap ranging from [8,16,32,64 ..`ZoneAllocator::MAX_ALLOC_SIZE`].
    /// We evenly distribute the pages allocated for each heap between the size classes. 
    pub fn init_individual_heap(key: usize, multiple_heaps: &mut MultipleHeaps) -> Result<(), &'static str> {

        let mut heap_end = multiple_heaps.end.lock();
        let mut heap_end_addr = *heap_end;

        let mapped_pages_per_size_class = PER_CORE_HEAP_INITIAL_SIZE_PAGES / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * HEAP_MAPPED_PAGES_SIZE_IN_PAGES);
        let mut zone_allocator = ZoneAllocator::new(key);

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
                let (mp, _action) = create_heap_mapping(heap_end_addr, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;

                let start_addr = mp.start_address().value();
                if start_addr % ObjectPage8k::SIZE != 0 {
                    return Err("MappedPages allocated for heap are not aligned on an 8k boundary");
                }
                multiple_heaps.extend_heap_mp(mp)?;
                let page = unsafe{ core::mem::transmute(start_addr) };
                zone_allocator.refill(layout, page)?;

                // update the end address of the heap
                heap_end_addr += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                // trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab]);
            }
        }

        // store the new end of the heap after this core has been initialized
        *heap_end = heap_end_addr;

        // store the newly created allocator in the multiple heaps object
        if let Some(_heap) = multiple_heaps.heaps.insert(key, LockedHeap(IrqSafeMutex::new(zone_allocator))) {
            return Err("New heap created with a previously used id");
        }
        trace!("Created heap {} with max alloc size: {} bytes", key, ZoneAllocator::MAX_ALLOC_SIZE);

        Ok(())
    }

} else {
    extern crate alloc;

    /// Initializes the heap given by `key`.
    /// There are 11 size classes in each heap ranging from [8,16,32,64 ..`ZoneAllocator::MAX_ALLOC_SIZE`].
    /// We evenly distribute the pages allocated for each heap between the size classes. 
    pub fn init_individual_heap(key: usize, multiple_heaps: &mut MultipleHeaps) -> Result<(), &'static str> {

        let mut heap_end = multiple_heaps.end.lock();
        let mut heap_end_addr = *heap_end;

        let mapped_pages_per_size_class = PER_CORE_HEAP_INITIAL_SIZE_PAGES / (ZoneAllocator::MAX_BASE_SIZE_CLASSES * HEAP_MAPPED_PAGES_SIZE_IN_PAGES);
        let mut zone_allocator = ZoneAllocator::new(key);

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
                let (mp, _action) = create_heap_mapping(heap_end_addr, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                let mapping = MappedPages8k::new(mp)?;
                // add page to the allocator
                zone_allocator.refill(layout, mapping)?;

                // update the end address of the heap
                // trace!("Added an object page {:#X} to slab of size {}", heap_end_addr, size);
                heap_end_addr += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
            }
        }

        // store the new end of the heap after this core has been initialized
        *heap_end = heap_end_addr;

        // store the newly created allocator in the multiple heaps object
        if let Some(_heap) = multiple_heaps.heaps.insert(key, LockedHeap(IrqSafeMutex::new(zone_allocator))) {
            return Err("New heap created with a previously used id");
        }
        trace!("Created heap {} with max alloc size: {} bytes", key, ZoneAllocator::MAX_ALLOC_SIZE);

        Ok(())
    }
}
} // end cfg_if for initialization functions


/// Returns the key that determines which heap will be currently used.
///
/// This implementation uses the current CPU ID as the key,
/// but this can easily be replaced with another value, e.g., Task ID.
#[inline(always)] 
fn get_key() -> usize {
    apic::current_cpu().value() as usize
}

// The LockedHeap struct definition changes depending on the slabmalloc version used.
// The safe version does not pass any lifetime parameter to the ZoneAllocator, while the unsafe and default versions do.
cfg_if! {
if #[cfg(safe_heap)] {
    #[repr(align(64))]
    struct LockedHeap (IrqSafeMutex<ZoneAllocator>);

    impl Deref for LockedHeap {
        type Target = IrqSafeMutex<ZoneAllocator>;
        fn deref(&self) -> &IrqSafeMutex<ZoneAllocator> {
            &self.0
        }
    }
} else {
    #[repr(align(64))]
    struct LockedHeap (IrqSafeMutex<ZoneAllocator<'static>>);

    impl Deref for LockedHeap {
        type Target = IrqSafeMutex<ZoneAllocator<'static>>;
        fn deref(&self) -> &IrqSafeMutex<ZoneAllocator<'static>> {
            &self.0
        }
    }
}
} // end cfg_if for LockedHeap versions


/// An allocator that contains multiple heaps. The heap that is used on each allocation is
/// determined by a key. Currently the apic id is used as the key.
pub struct MultipleHeaps{
    /// the per-core heaps
    heaps: HashMap<usize,LockedHeap>,
    /// Red-black tree to store large allocations
    #[cfg(not(unsafe_large_allocations))]    
    large_allocations: IrqSafeMutex<RBTree<LargeAllocationAdapter>>,
    /// We currently don't return memory back to the OS. Because of this all memory in the heap is contiguous
    /// and extra memory for the heap is always allocated from the end.
    /// The Mutex also serves the purpose of helping to synchronize new allocations.
    end: IrqSafeMutex<VirtualAddress>, 
    /// The mapped pages for the unsafe heap are stored here so that they are not dropped and unmapped.
    #[cfg(unsafe_heap)]    
    mp: Once<IrqSafeMutex<MappedPages>>
}

// The grow_heap() function for the MultipleHeaps changes depending on the slabmalloc version used.
//
// In the default version, MappedPages8k objects are passed to the heap that needs to be grown.
// In the unsafe version, the new heap mapping is merged into the heap MappedPages object in the kernel mmi
// and then a reference to the starting address is passed to the heap that needs to be grown.
// In the safe version, an Err is returned since the heap is statically sized.
cfg_if! {
if #[cfg(unsafe_heap)] {
    impl MultipleHeaps {
        pub fn empty() -> MultipleHeaps {
            MultipleHeaps{
                heaps: HashMap::new(),

                #[cfg(not(unsafe_large_allocations))]
                large_allocations: IrqSafeMutex::new(RBTree::new(LargeAllocationAdapter::new())),

                end: IrqSafeMutex::new(VirtualAddress::new_canonical(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE)),

                mp: Once::new()
            }
        }

        /// Called when a call to allocate() returns a null pointer. The following steps are used to recover memory:
        /// (1) Pages are first taken from another heap.
        /// (2) If the above fails, then more pages are allocated from the OS.
        /// 
        /// An Err is returned if there is no more memory to be allocated in the heap memory area.
        /// 
        /// # Arguments
        /// * `layout`: layout.size will determine which allocation size the retrieved pages will be used for. 
        /// * `heap_to_grow`: heap that needs to grow.
        fn grow_heap(&self, layout: Layout, heap_to_grow: &LockedHeap) -> Result<(), &'static str> {
            // (1) Try to retrieve a page from the another heap
            for heap_ref in self.heaps.values() {
                if let Some((mp, _giving_heap_id)) = heap_ref.try_lock().and_then(|mut giving_heap| 
                    giving_heap.retrieve_empty_page(EMPTY_PAGES_THRESHOLD).map(|mp| (mp, giving_heap.heap_id))
                ) {
                    info!("Added page from another heap {} to heap {}", _giving_heap_id, heap_to_grow.lock().heap_id);
                    return heap_to_grow.lock().refill(layout, mp);
                }
            }

            // (2) Allocate page from the OS
            let mut heap_end = self.end.lock();
            for _ in 0..HEAP_GROWTH_AMOUNT {
                let (mp, _action) = create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                let start_addr = mp.start_address().value();
                self.extend_heap_mp(mp)?;
                let page = unsafe { core::mem::transmute(start_addr) };
                info!("grow_heap:: Allocated page(s) at {:X?} to refill heap {} for layout size: {}, prior heap_end: {:#X}", 
                    start_addr, heap_to_grow.lock().heap_id, layout.size(), *heap_end
                );
                *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                heap_to_grow.lock().refill(layout, page)?;
            }
            Ok(())
        } 

        /// Merge mapped pages `mp` with the heap mapped pages.
        /// 
        /// # Warning
        /// The new mapped pages must start from the virtual address that the current heap mapped pages end at.
        fn extend_heap_mp(&self, mp: MappedPages) -> Result<(), &'static str> {
            if let Some(heap_mp) = self.mp.get() {
                heap_mp.lock().merge(mp).map_err(|(e, _mp)| e)?;
            } else {
                self.mp.call_once(|| IrqSafeMutex::new(mp));
            }
            Ok(())
        }
    }
} else if #[cfg(safe_heap)] {
    impl MultipleHeaps {
        pub fn empty() -> MultipleHeaps {
            MultipleHeaps{
                heaps: HashMap::new(),

                #[cfg(not(unsafe_large_allocations))]
                large_allocations: IrqSafeMutex::new(RBTree::new(LargeAllocationAdapter::new())),

                end: IrqSafeMutex::new(VirtualAddress::new_canonical(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE))
            }
        }

        /// Called when a call to allocate() returns a null pointer. The following steps are used to recover memory:
        /// (1) Pages are first taken from another heap.
        /// (2) If the above fails, then more pages are allocated from the OS.
        /// 
        /// An Err is returned if there is no more memory to be allocated in the heap memory area or if the heap page limit is reached.
        /// 
        /// # Arguments
        /// * `layout`: layout.size will determine which allocation size the retrieved pages will be used for. 
        /// * `heap_to_grow`: heap that needs to grow.
        fn grow_heap(&self, layout: Layout, heap_to_grow: &LockedHeap) -> Result<(), &'static str> {
            // (1) Try to retrieve a page from the another heap
            for heap_ref in self.heaps.values() {
                if let Some((mp, _giving_heap_id)) = heap_ref.try_lock().and_then(|mut giving_heap| 
                    giving_heap.retrieve_empty_page(EMPTY_PAGES_THRESHOLD).map(|mp| (mp, giving_heap.heap_id))
                ) {
                    info!("Added page from another heap {} to heap {}", _giving_heap_id, heap_to_grow.lock().heap_id);
                    return heap_to_grow.lock().refill(layout, mp);
                }
            }

            // (2) Allocate page from the OS
            let mut heap_end = self.end.lock();
            for _ in 0..HEAP_GROWTH_AMOUNT {
                let (mp, _action) = create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                let mp = MappedPages8k::new(mp)?;
                info!("grow_heap:: Allocated page(s) at {:X?} to refill heap {} for layout size: {}, prior heap_end: {:#X}", 
                    mp.start_address(), heap_to_grow.lock().heap_id, layout.size(), *heap_end
                );
                *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                heap_to_grow.lock().refill(layout, mp)?;
            }
            Ok(())
        }  
    }

} else {
    impl MultipleHeaps {
        pub fn empty() -> MultipleHeaps {
            MultipleHeaps{
                heaps: HashMap::new(),

                #[cfg(not(unsafe_large_allocations))]
                large_allocations: IrqSafeMutex::new(RBTree::new(LargeAllocationAdapter::new())),

                end: IrqSafeMutex::new(VirtualAddress::new_canonical(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE))
            }
        }

        /// Called when a call to allocate() returns a null pointer. The following steps are used to recover memory:
        /// (1) Pages are first taken from another heap.
        /// (2) If the above fails, then more pages are allocated from the OS.
        /// 
        /// An Err is returned if there is no more memory to be allocated in the heap memory area.
        /// 
        /// # Arguments
        /// * `layout`: layout.size will determine which allocation size the retrieved pages will be used for. 
        /// * `heap_to_grow`: heap that needs to grow.
        fn grow_heap(&self, layout: Layout, heap_to_grow: &LockedHeap) -> Result<(), &'static str> {
            // (1) Try to retrieve a page from the another heap
            for heap_ref in self.heaps.values() {
                if let Some((mp, _giving_heap_id)) = heap_ref.try_lock().and_then(|mut giving_heap| 
                    giving_heap.retrieve_empty_page(EMPTY_PAGES_THRESHOLD).map(|mp| (mp, giving_heap.heap_id))
                ) {
                    info!("Added page from another heap {} to heap {}", _giving_heap_id, heap_to_grow.lock().heap_id);
                    return heap_to_grow.lock().refill(layout, mp);
                }
            }

            // (2) Allocate page from the OS
            let mut heap_end = self.end.lock();
            for _ in 0..HEAP_GROWTH_AMOUNT {
                let (mp, _action) = create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?;
                let mp = MappedPages8k::new(mp)?;
                info!("grow_heap:: Allocated page(s) at {:X?} to refill heap {} for layout size: {}, prior heap_end: {:#X}", 
                    mp.start_address(), heap_to_grow.lock().heap_id, layout.size(), *heap_end
                );
                *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
                heap_to_grow.lock().refill(layout, mp)?;
            }
            Ok(())
        }  
    }
}
} // end cfg_if for MultipleHeaps impl


unsafe impl GlobalAlloc for MultipleHeaps {

    /// Allocates the given `layout` from the heap of the core the task is currently running on.
    /// If the per-core heap is not initialized, then an error is returned.
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // allocate a large object by directly obtaining mapped pages from the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            #[cfg(not(unsafe_large_allocations))]
            return allocate_large_object(
                layout, 
                &mut self.large_allocations.lock()
            );

            #[cfg(unsafe_large_allocations)]
            return allocate_large_object(layout);
        }

        // For regular-sized allocations, we first try to allocated from "our" heap, 
        // which is currently the per-core heap for the current CPU core. 
        let our_heap = self.heaps.get(&get_key()).expect("Multiple Heaps: heap is not initialized!");
        if let Ok(ptr) = { our_heap.lock().allocate(layout) } {
            return ptr.as_ptr();
        };
        // If it fails the first time, we try to grow the heap and then try again. 
        // We must not hold any heap locks while doing so, since growing the heap may result in
        // additional heap allocation by virtue of allocating more pages. 
        self.grow_heap(layout, our_heap)
            .and_then(|_| our_heap.lock().allocate(layout))    // try again
            .map(|nn| nn.as_ptr())                             // convert to raw ptr
            .unwrap_or(ptr::null_mut())
    }

    /// Deallocates the memory at the address given by `ptr`.
    /// Memory is returned to the per-core heap it was allocated from.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {   
        // deallocate a large object by directly returning mapped pages to the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            #[cfg(not(unsafe_large_allocations))]
            return deallocate_large_object(
                ptr, 
                layout, 
                &mut self.large_allocations.lock()
            );

            #[cfg(unsafe_large_allocations)]
            return deallocate_large_object(
                ptr, 
                layout
            );

        }     
        // find the starting address of the object page this block belongs to
        let page_addr = (ptr as usize) & !(ObjectPage8k::SIZE - 1);
        // find the heap id
        let id = *((page_addr as *mut u8).add(ObjectPage8k::HEAP_ID_OFFSET) as *mut usize);
        let mut heap = self.heaps.get(&id).expect("Multiple Heaps: Heap not initialized").lock();
        heap.deallocate(NonNull::new_unchecked(ptr), layout).expect("Couldn't deallocate");
    }
}



cfg_if! {
if #[cfg(unsafe_large_allocations)] {
    /// Any memory request greater than MAX_ALLOC_SIZE is satisfied through a request to the OS.
    /// The pointer to the beginning of the newly allocated pages is returned.
    /// The MappedPages object returned by that request is written to the end of the memory allocated.
    /// 
    /// # Warning
    /// This function should only be used by an allocator in conjunction with [`deallocate_large_object()`](fn.deallocate_large_object.html)
    fn allocate_large_object(layout: Layout) -> *mut u8 {
        // the mapped pages must have additional memory on the end where we can store the mapped pages object
        let allocation_size = layout.size() + core::mem::size_of::<MappedPages>();

        if let Ok(mapping) = create_mapping(allocation_size, HEAP_FLAGS) {
            let ptr = mapping.start_address().value() as *mut u8;
            // This is safe since we ensure that the memory allocated includes space for the MappedPages object,
            // and since the corresponding deallocate function makes sure to retrieve the MappedPages object and drop it.
            unsafe{ (ptr.offset(layout.size() as isize) as *mut MappedPages).write(mapping); }
            // trace!("Allocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
            ptr
        } else {
            error!("Could not create mapping for a large object in the heap");
            ptr::null_mut()
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

} else {
    extern crate intrusive_collections;

    use intrusive_collections::{intrusive_adapter,RBTree, RBTreeLink, KeyAdapter, PointerOps};


    /// The links for the RBTree used to store large allocations
    struct LargeAllocation {
        link: RBTreeLink,
        mp: MappedPages
    }

    // Our design depends on the fact that on the large allocation path, only objects smaller than the max allocation size will be allocated from the heap.
    // Otherwise we will have a recursive loop of large allocations.
    const _: () = assert!(core::mem::size_of::<LargeAllocation>() < ZoneAllocator::MAX_ALLOC_SIZE);

    intrusive_adapter!(LargeAllocationAdapter = Box<LargeAllocation>: LargeAllocation { link: RBTreeLink });

    /// Defines the key which will be used to search for elements in the RBTree.
    /// Here it is the starting address of the allocation.
    impl<'a> KeyAdapter<'a> for LargeAllocationAdapter {
        type Key = usize;
        fn get_key(&self, value: &'a <Self::PointerOps as PointerOps>::Value) -> usize {
            value.mp.start_address().value()
        }
    }

    /// Any memory request greater than `ZoneAllocator::MAX_ALLOC_SIZE` is satisfied through a request to the OS.
    /// The pointer to the beginning of the newly allocated pages is returned.
    /// The MappedPages object returned by that request is stored in an RB-tree
    /// 
    /// # Warning
    /// This function should only be used by an allocator in conjunction with [`deallocate_large_object()`](fn.deallocate_large_object.html)
    fn allocate_large_object(layout: Layout, map: &mut RBTree<LargeAllocationAdapter>) -> *mut u8 {
        if let Ok(mp) = create_mapping(layout.size(), HEAP_FLAGS) {
            let ptr = mp.start_address().value();
            let link = Box::new(LargeAllocation {
                link: RBTreeLink::new(),
                mp
            });
            map.insert(link);
            // trace!("Allocated a large object of {} bytes at address: {:#X}", layout.size(), ptr as usize);
            ptr as *mut u8

        } else {
            error!("Could not create mapping for a large object in the heap");
            ptr::null_mut()
        }
        
    }

    /// Any memory request greater than `ZoneAllocator::MAX_ALLOC_SIZE` was created by requesting a MappedPages object from the OS,
    /// and now the MappedPages object will be retrieved from the RB-tree and dropped to deallocate the memory referenced by `ptr`.
    /// 
    /// # Warning
    /// This function should only be used by an allocator in conjunction with [`allocate_large_object()`](fn.allocate_large_object.html) 
    fn deallocate_large_object(ptr: *mut u8, _layout: Layout, map: &mut RBTree<LargeAllocationAdapter>) {
        let _mp = map.find_mut(&(ptr as usize)).remove()
            .expect("Invalid ptr was passed to deallocate_large_object. There is no such mapping stored");
        // trace!("Deallocated a large object of {} bytes at address: {:#X} {:#X}", _layout.size(), ptr as usize, _mp.mp.start_address());
    }

}
} // end cfg_if for large allocation variations



