//! The global allocator for the system. 
//! It starts off as a single fixed size allocator, and when a more complex heap is set up
//! the allocate and deallocate functions are updated.

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
extern crate multiple_heaps;

use core::ptr::{self, NonNull};
use alloc::alloc::{GlobalAlloc, Layout};
use memory::{EntryFlags, VirtualAddress, PageTable, AreaFrameAllocator, PageRange};
use kernel_config::memory::PAGE_SIZE;
use irq_safety::MutexIrqSafe;
use core::ops::Add;
use spin::Once;
use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use core::ops::DerefMut;
use multiple_heaps::{MultipleHeaps, merge_initial_heap, multiple_heaps};

#[global_allocator]
static ALLOCATOR: Heap<MultipleHeaps> = Heap::empty();


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

/// Transfers mapped pages belonging to the initial allocator to the first multiple heap
/// and sets the multiple heaps as the default allocator.
/// Only call this function when the multiple heaps are ready to be used.
pub fn switch_to_multiple_heaps() -> Result<(), &'static str> {
    // lock the allocator so that no allocation or deallocation can take place
    let mut initial_allocator = ALLOCATOR.initial_allocator.lock();

    // switch out the initial allocator with an empty heap
    let mut zone_allocator = ZoneAllocator::new();
    core::mem::swap(&mut *initial_allocator, &mut zone_allocator);

    // transfer initial heap to the first multiple heap
    merge_initial_heap(zone_allocator)?;

    //set the multiple heaps as the default allocator
    ALLOCATOR.allocator.call_once(|| multiple_heaps());

    Ok(())
}

/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, and when 
/// a more complex heap is initialized the new allocate and deallocate functions are set.
pub struct Heap <'a, T: GlobalAlloc> {
    initial_allocator: MutexIrqSafe<ZoneAllocator<'static>>, 
    allocator: Once<&'a T>
}


impl <'a, T: GlobalAlloc> Heap <'a, T> {
    pub const fn empty() -> Heap<'a, T> {
        Heap{
            initial_allocator: MutexIrqSafe::new(ZoneAllocator::new()),
            allocator: Once::new()
        }
    }
}

unsafe impl<'a, T: GlobalAlloc> GlobalAlloc for Heap<'a, T> {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.allocator.try() {
            Some(allocator) => {
                allocator.alloc(layout)
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
        match self.allocator.try() {
            Some(allocator) => {
                allocator.dealloc(ptr, layout)
            }
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                self.initial_allocator.lock().deallocate(ptr, layout).expect("Deallocation failed!");
            }
        }
    }

}