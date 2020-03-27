//! An implementation of a per core heap

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
use memory::{MappedPages, create_mapping, create_mapping_8k_aligned, EntryFlags};
use heap_irq_safe::{IrqSafeHeap, create_object_page, MAX_ALLOC_SIZE, OBJECT_PAGE_SIZE_BYTES, OBJECT_PAGE_SIZE_PAGES, ObjectPage8k, MAX_SIZE_CLASSES};
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

/// A heap must have greater than this number of empty pages to return one.
pub const RETURN_THRESHOLD: usize = MAX_SIZE_CLASSES * 2;

/// Allocates pages that are aligned on an 8k boundary.
/// That is essential for an object page that is added to the heap.
pub fn allocate_8k_aligned_mapped_pages(size_in_pages: usize) -> Option<MappedPages> {
    create_mapping_8k_aligned(size_in_pages, EntryFlags::WRITABLE).ok()
}

pub struct MultipleHeaps{
    heaps: [IrqSafeHeap; MAX_HEAPS],
}

impl MultipleHeaps {
    pub const fn empty() -> MultipleHeaps {
        MultipleHeaps{
            heaps: [IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty()],
        }
    }

    /// Retrieves an empty page from the heap which has the maximum number of empty pages,
    /// if the maximum is greater than a threshold.
    fn return_page(&self) -> Option<&'static mut ObjectPage8k<'static>> {
        // find heap with maximum number of empty pages
        let idx = self.heap_index_with_max_empty_pages();
        let max_pages = self[idx].empty_pages();
        if max_pages > RETURN_THRESHOLD {
            unsafe { self.heaps[idx].return_page() }
        }
        else {
            None
        }
    }

    fn heap_index_with_max_empty_pages(&self) -> usize {
        self.iter().enumerate().max_by_key(|&(_i, val)| val.empty_pages()).unwrap().0 //unwrap here since we know the heap is not empty
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
                    if let Some(mapped_pages) = allocate_8k_aligned_mapped_pages(OBJECT_PAGE_SIZE_PAGES) {
                        if let Ok(page) = create_object_page(mapped_pages.start_address().value()) {
                            match self[heap_id].refill(layout, page) {
                                Ok(()) => {
                                    trace!("Added an object page to the heap");
                                    ptr = self[heap_id].alloc(layout);
                                    
                                    // right now we forget any extra mapped pages added to the heap since 
                                    // we aren't currently returning memory to the system.
                                    // TODO: allocate mapped pages from within the KERNEL_HEAP range and update the kernel heap vma
                                    core::mem::forget(mapped_pages);
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
