//! An implementation of an allocator that uses per-core heaps
//! 
//! The allocator is made up of multiple heaps. The heap algorithm used by each individual heap is determined in the heap_irq_safe crate. 
//! 
//! This allocator decides:
//!  * If the requested size is large enough to allocate pages directly from the OS
//!  * If the requested size is small, which heap to actually allocate/deallocate from
//!  * How to deal with OOM errors returned by a heap
//! 
//! Any memory request greater than 8104 bytes (8192 bytes - 88 bytes of metadata) is satisfied through a request for mapped pages from the kernel.
//! All other requests are satified through the per-core heaps.
//! 
//! The heap which will be used on allocation is determined by the cpu that the task is running on.
//! On deallocation of a block, the heap id is retrieved from metadata at the end of the page which contains the block.
//! 
//! When a per-core heap runs out of memory, memory is first requested from other per-core heaps if they have empty (unused) pages.
//! If they don't, then more memory is allocated from the kernel's heap area.
//!  
//! The maximum number of heaps is configured in the kernel configuration variable, MAX_HEAPS.

#![feature(const_fn)]
#![feature(allocator_api)]

#![no_std]

extern crate alloc;
extern crate linked_list_allocator;
extern crate irq_safety; 
extern crate spin;
extern crate block_allocator;
extern crate raw_cpuid;
#[macro_use] extern crate log;
extern crate memory;
extern crate heap_irq_safe;
extern crate kernel_config;

use core::ops::Deref;
use alloc::alloc::{GlobalAlloc, Layout};
use raw_cpuid::CpuId;
use memory::{MappedPages, create_mapping, EntryFlags, allocate_heap_pages};
use heap_irq_safe::{IrqSafeHeap, create_object_page, MAX_ALLOC_SIZE, OBJECT_PAGE_SIZE_BYTES, ObjectPage8k, MAX_SIZE_CLASSES};
use kernel_config::memory::MAX_HEAPS;

#[global_allocator]
static ALLOCATOR: MultipleHeaps = MultipleHeaps::empty();


/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
/// Divides the memory evenly between the heaps.
pub fn init(start_virt_addr: usize, size_in_bytes: usize) -> Result<(), &'static str>{
    let bytes_per_heap = size_in_bytes / MAX_HEAPS;

    if bytes_per_heap % OBJECT_PAGE_SIZE_BYTES != 0 {
        return Err("Heap memory does not divide evenly on an object page boundary");
    }

    for i in 0..MAX_HEAPS {
        unsafe {
            ALLOCATOR[i].init(start_virt_addr + i*bytes_per_heap, bytes_per_heap, Some(i))?
        }
    }

    Ok(())
}

/// A heap must have greater than this number of empty object pages to return one.
pub const RETURN_THRESHOLD: usize = MAX_SIZE_CLASSES * 2;

pub struct MultipleHeaps{
    heaps: [IrqSafeHeap; MAX_HEAPS],
}

impl MultipleHeaps {
    pub const fn empty() -> MultipleHeaps {
        MultipleHeaps{
            heaps: [IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty()],
        }
    }

    /// Retrieves an empty object page from the heap which has the maximum number of empty pages,
    /// if the maximum is greater than a threshold.
    fn return_page(&self) -> Option<&'static mut ObjectPage8k<'static>> {
        // find heap with maximum number of empty pages
        let idx = self.heap_with_max_empty_pages();
        let max_pages = self[idx].empty_pages();
        if max_pages > RETURN_THRESHOLD {
            unsafe { self.heaps[idx].return_page() }
        }
        else {
            None
        }
    }

    /// The index for the heap with the maximum number of empty pages
    fn heap_with_max_empty_pages(&self) -> usize {
        self.iter().enumerate().max_by_key(|&(_i, val)| val.empty_pages()).unwrap().0 //unwrap here since we know the heap array is not empty
    }
}

impl Deref for MultipleHeaps {
    type Target = [IrqSafeHeap; MAX_HEAPS];

    fn deref(&self) -> &[IrqSafeHeap; MAX_HEAPS] {
        &self.heaps
    }
}

unsafe impl GlobalAlloc for MultipleHeaps {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();

        // directly allocate memory from the OS
        if size > MAX_ALLOC_SIZE {
            let size_mp = core::mem::size_of::<MappedPages>();
            let mut ptr = 0 as *mut u8;

            // the mapped pages must have additional memory on the end where we can store the mapped pages object
            if let Ok(mapped_pages) = create_mapping(size + size_mp, EntryFlags::WRITABLE) {
                ptr = mapped_pages.start_address().value() as *mut u8;
                let ptr_mp = ptr.offset(size as isize) as *mut MappedPages;
                ptr_mp.write(mapped_pages);

                trace!("Allocated a large object of {} bytes at address: {:#X}", size, ptr as usize);
            }
            ptr
        }
        // allocate from the heap
        else {
            let heap_id = CpuId::new().get_feature_info().expect("Could not retrieve cpuid").initial_local_apic_id() as usize % MAX_HEAPS;
            let mut ptr = self[heap_id].alloc(layout);

            // the heap does not have memory for this layout
            // and not enough empty pages to borrow from within the heap
            if ptr == (0 as *mut u8) {
                // first try to retrive an empty page from other heaps
                if let Some(page) = self.return_page() {
                    page.clear_metadata();
                    match self[heap_id].refill(layout, page) {
                        Ok(()) => {
                            trace!("transferred a page between heaps");
                            ptr = self[heap_id].alloc(layout);
                        }
                        Err(_x) => {
                            error!("Could not refill heap");
                        }
                    }
                }
                // There are no available empty pages so we have to allocate memory from the OS
                else {
                    if let Ok(addr) = allocate_heap_pages(OBJECT_PAGE_SIZE_BYTES) {
                        if let Ok(page) = create_object_page(addr.value()) {
                            match self[heap_id].refill(layout, page) {
                                Ok(()) => {
                                    trace!("Added an object page to the heap at address: {:#X}", addr);
                                    ptr = self[heap_id].alloc(layout);
                                }
                                Err(_x) => {
                                    error!("Could not refill heap");
                                }
                            }
                        }
                    }
                }
            }

            ptr    
        }    
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();

        // memory was directly allocated from the OS so we need to retrieve the mapped pages and drop
        if size > MAX_ALLOC_SIZE {
            let ptr_mp = ptr.offset(size as isize) as *const MappedPages;
            let _mp = core::ptr::read(ptr_mp); 
            trace!("Deallocated a large object of {} bytes at address: {:#X}", size, ptr as usize);
        }
        // return memory to the allocator
        else {
            // find the starting address of the object page this block belongs to
            let page_addr = (ptr as usize) & !(OBJECT_PAGE_SIZE_BYTES - 1);
            let page = create_object_page(page_addr);
            assert!(page.is_ok()); 

            // find the heap id from the page's metadata
            let heap_id = page.unwrap().heap_id;
            self[heap_id].dealloc(ptr, layout)
        }
    }
}
