//! An implementation of an allocator that uses multiple heaps. The heap that will be used on each allocation is determined by a key.
//! Right now we use the apic id as the key, so that we have per-core heaps.
//! 
//! The heaps are ZoneAllocators (given in the slabmalloc crate). Each ZoneAllocator maintains 11 separate "slab allocators" for sizes
//! 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096 and 8072 (8KiB - bytes of metadata) bytes. 8072 is the maximum allocation size given by `ZoneAllocator::MAX_ALLOC_SIZE`.
//! The slab allocator maintains a list of MappedPages8k, and uses the underlying pages (allocable pages) to allocates objects of the same size. 
//! The allocable pages are 8 KiB (`MappedPages8K::SIZE`), and have metadata stored in the last 120 bytes (`MappedPages8K::METADATA_SIZE`).
//! The metadata includes a heap id, a MappedPages8k object that holds the next page in the page list, and a
//! bitmap to keep track of allocations. The maximum allocation size can change if the size of the objects in the metadata change. If that happens it will be automatically
//! reflected in the constants `ZoneAllocator::MAX_ALLOC_SIZE` and `MappedPages8K::METADATA_SIZE`
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

#![feature(const_fn)]
#![feature(allocator_api)]
#![no_std]

extern crate alloc;
extern crate irq_safety; 
#[macro_use] extern crate log;
extern crate memory;
extern crate kernel_config;
extern crate slabmalloc;
extern crate apic;
extern crate heap;
extern crate intrusive_collections;
extern crate hashbrown;
#[macro_use] extern crate static_assertions;


use core::ptr::NonNull;
use alloc::alloc::{GlobalAlloc, Layout};
use alloc::{
    boxed::Box
};
use hashbrown::HashMap;

use memory::{MappedPages, VirtualAddress, get_frame_allocator_ref, get_kernel_mmi_ref, PageRange, create_mapping};
use kernel_config::memory::{PAGE_SIZE, KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE, KERNEL_HEAP_MAX_SIZE};
use irq_safety::MutexIrqSafe;
use slabmalloc::{ZoneAllocator, Allocator, MappedPages8k};
use core::ops::{Add, Deref, DerefMut};
use core::ptr;
use heap::HEAP_FLAGS;
use intrusive_collections::{intrusive_adapter,RBTree, RBTreeLink, KeyAdapter, PointerOps};


/// The size of each MappedPages Object that is allocated for the per-core heaps, in bytes.
/// We curently work with 8KiB, so that the per core heaps can allocate objects up to `ZoneAllocator::MAX_ALLOC_SIZE`.  
const HEAP_MAPPED_PAGES_SIZE_IN_BYTES: usize = MappedPages8k::SIZE;

/// The size of each MappedPages Object that is allocated for the per-core heaps, in pages.
/// We curently work with 2 pages, so that the per core heaps can allocate objects up to `ZoneAllocator::MAX_ALLOC_SIZE`.   
const HEAP_MAPPED_PAGES_SIZE_IN_PAGES: usize = MappedPages8k::SIZE / PAGE_SIZE;

/// When an OOM error occurs, before allocating more memory from the OS, we first try to see if there are unused(empty) pages 
/// within the per-core heaps that can be moved to other heaps. To prevent any heap from completely running out of memory we 
/// set this threshold value. A heap must have greater than this number of empty mapped pages to return one for use by other heaps.
const EMPTY_PAGES_THRESHOLD: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES * 2;

/// The number of pages each size class in the ZoneAllocator is initialized with.
const PAGES_PER_SIZE_CLASS: usize = 24; 

/// Starting size of each per-core heap. It's approximately 1 MiB.
pub const PER_CORE_HEAP_INITIAL_SIZE_PAGES: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES *  PAGES_PER_SIZE_CLASS;


/// Creates and initializes the multiple heaps using the apic id as the key, which is mapped to a heap.
/// If we want to change the value the heap id is based on, we would substitute 
/// the lapic iterator with an iterator containing the desired keys.
fn initialize_multiple_heaps() -> Result<MultipleHeaps, &'static str> {
    let mut multiple_heaps = MultipleHeaps::empty();

    for (apic_id, _lapic) in apic::get_lapics().iter() {
        init_individual_heap(*apic_id as usize, &mut multiple_heaps)?;
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
fn create_heap_mapping(starting_address: VirtualAddress, size_in_bytes: usize) -> Result<MappedPages, &'static str> {
    if (starting_address.value() + size_in_bytes) >  (KERNEL_HEAP_START + KERNEL_HEAP_MAX_SIZE) {
        return Err("Heap memory limit has been reached");
    }

    let kernel_mmi_ref = get_kernel_mmi_ref().ok_or("create_heap_mapping(): KERNEL_MMI was not yet initialized!")?;
    let mut kernel_mmi = kernel_mmi_ref.lock();

    let mut frame_allocator = get_frame_allocator_ref()
        .ok_or("create_heap_mapping(): couldnt get FRAME_ALLOCATOR")?
        .lock();

    let pages = PageRange::from_virt_addr(starting_address, size_in_bytes);
    let heap_flags = HEAP_FLAGS;
    let mp = kernel_mmi.page_table.map_pages(pages, heap_flags, frame_allocator.deref_mut())?;

    // trace!("Allocated heap pages at: {:#X}", starting_address);

    Ok(mp)
}


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

            // create the mapped pages starting from the previous end of the heap, and ensure they fit the heap requirements
            // by converting them to a MappedPages8k
            let mapping = MappedPages8k::new(create_heap_mapping(heap_end_addr, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?)?;

            // add page to the allocator
            zone_allocator.refill(layout, mapping)?; 

            // update the end address of the heap
            // trace!("Added an object page {:#X} to slab of size {}", heap_end_addr, size);
            heap_end_addr = heap_end_addr.add(HEAP_MAPPED_PAGES_SIZE_IN_BYTES);
        }
    }

    // store the new end of the heap after this core has been initialized
    *heap_end = heap_end_addr;

    // store the newly created allocator in the multiple heaps object
    if let Some(_heap) = multiple_heaps.heaps.insert(key, LockedHeap(MutexIrqSafe::new(zone_allocator))) {
        return Err("New heap created with a previously used id");
    }
    trace!("Created heap {} with max alloc size: {} bytes", key, ZoneAllocator::MAX_ALLOC_SIZE);

    Ok(())
}


/// Returns the key which determines the heap that will be used.
/// Currently we use the apic id as the key, but we can replace it with some
/// other value e.g. task id
#[inline(always)] 
fn get_key() -> usize {
    apic::get_my_apic_id() as usize
}


/// The links for the RBTree used to store large allocations
struct LargeAllocation {
    link: RBTreeLink,
    mp: MappedPages
}

// Our design depends on the fact that on the large allocation path, only objects smaller than the max allocation size will be allocated from the heap.
// Otherwise we will have a recursive loop of large allocations.
const_assert!(core::mem::size_of::<LargeAllocation>() < ZoneAllocator::MAX_ALLOC_SIZE); 

intrusive_adapter!(LargeAllocationAdapter = Box<LargeAllocation>: LargeAllocation { link: RBTreeLink });

/// Defines the key which will be used to search for elements in the RBTree.
/// Here it is the starting address of the allocation.
impl<'a> KeyAdapter<'a> for LargeAllocationAdapter {
    type Key = usize;
    fn get_key(&self, value: &'a <Self::PointerOps as PointerOps>::Value) -> usize {
        value.mp.start_address().value()
    }
}

#[repr(align(64))]
struct LockedHeap (MutexIrqSafe<ZoneAllocator>);

impl Deref for LockedHeap {
    type Target = MutexIrqSafe<ZoneAllocator>;
    fn deref(&self) -> &MutexIrqSafe<ZoneAllocator> {
        &self.0
    }
}


/// An allocator that contains multiple heaps. The heap that is used on each allocation is
/// determined by a key. Currently the apic id is used as the key.
pub struct MultipleHeaps{
    /// the per-core heaps
    heaps: HashMap<usize,LockedHeap>,
    /// Red-black tree to store large allocations
    large_allocations: MutexIrqSafe<RBTree<LargeAllocationAdapter>>,
    /// We currently don't return memory back to the OS. Because of this all memory in the heap is contiguous
    /// and extra memory for the heap is always allocated from the end.
    /// The Mutex also serves the purpose of helping to synchronize new allocations.
    end: MutexIrqSafe<VirtualAddress>, 
}

impl MultipleHeaps {
    pub fn empty() -> MultipleHeaps {
        MultipleHeaps{
            heaps: HashMap::new(),
            large_allocations: MutexIrqSafe::new(RBTree::new(LargeAllocationAdapter::new())),
            end: MutexIrqSafe::new(VirtualAddress::new_canonical(KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE))
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
    /// * `heap`: heap that needs to grow.
    fn grow_heap(&self, layout: Layout, heap: &mut ZoneAllocator) -> Result<(), &'static str> {
        // (1) Try to retrieve a page from the another heap
        for locked_heap in self.heaps.values() {
            if let Some(mp) = locked_heap.try_lock().and_then(|mut giving_heap| giving_heap.retrieve_empty_page(EMPTY_PAGES_THRESHOLD)) {
                info!("Added page from another heap to heap: {}", heap.heap_id);
                return heap.refill(layout, mp);
            }
        }
        // (2) Allocate page from the OS
        let mut heap_end = self.end.lock();
        let mp = MappedPages8k::new(create_heap_mapping(*heap_end, HEAP_MAPPED_PAGES_SIZE_IN_BYTES)?)?;
        info!("grow_heap:: Allocated a page to refill core heap {} for size :{} at address: {:#X}", heap.heap_id, layout.size(), *heap_end);
        *heap_end += HEAP_MAPPED_PAGES_SIZE_IN_BYTES;
        heap.refill(layout, mp)
    }

}


unsafe impl GlobalAlloc for MultipleHeaps {

    /// Allocates the given `layout` from the heap of the core the task is currently running on.
    /// If the per-core heap is not initialized, then an error is returned.
    #[inline(always)]    
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // allocate a large object by directly obtaining mapped pages from the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return allocate_large_object(
                layout, 
                &mut self.large_allocations.lock()
            )
        }

        let id = get_key();
        let mut heap = self.heaps.get(&id).expect("Multiple Heaps: heap is not initialized!").lock();
        heap.allocate(layout)
            .or_else(|_e| self.grow_heap(layout, &mut heap).and_then(|_| heap.allocate(layout)))
            .map(|allocation| allocation.as_ptr()).unwrap_or(ptr::null_mut())
    }

    /// Deallocates the memory at the address given by `ptr`.
    /// Memory is returned to the per-core heap it was allocated from.
    #[inline(always)]    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {   
        // deallocate a large object by directly returning mapped pages to the OS
        if layout.size() > ZoneAllocator::MAX_ALLOC_SIZE {
            return deallocate_large_object(
                ptr, 
                layout, 
                &mut self.large_allocations.lock()
            )
        }     
        // find the starting address of the object page this block belongs to
        let page_addr = (ptr as usize) & !(MappedPages8k::SIZE - 1);
        // find the heap id
        let id = *((page_addr as *mut u8).offset(MappedPages8k::HEAP_ID_OFFSET as isize) as *mut usize);
        let mut heap = self.heaps.get(&id).expect("Multiple Heaps: Heap not initialized").lock();
        heap.deallocate(NonNull::new_unchecked(ptr), layout).expect("Couldn't deallocate");
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
            mp: mp
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
/// and now the MappedPages object will be retrieved and dropped to deallocate the memory referenced by `ptr`.
/// 
/// # Warning
/// This function should only be used by an allocator in conjunction with [`allocate_large_object()`](fn.allocate_large_object.html) 
fn deallocate_large_object(ptr: *mut u8, _layout: Layout, map: &mut RBTree<LargeAllocationAdapter>) {
    let _mp = map.find_mut(&(ptr as usize)).remove()
        .expect("Invalid ptr was passed to deallocate_large_object. There is no such mapping stored");
    // trace!("Deallocated a large object of {} bytes at address: {:#X} {:#X}", _layout.size(), ptr as usize, _mp.mp.start_address());
}