// Copyright 2016 Philipp Oppermann. See the README.md
// file at the top-level directory of this distribution.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// modified by Kevin Boos

#![feature(const_fn)]
#![feature(allocator_api)]

#![no_std]

extern crate alloc;
extern crate irq_safety; 
extern crate raw_cpuid;
#[macro_use] extern crate log;
extern crate slabmalloc;
extern crate kernel_config;

use alloc::alloc::{GlobalAlloc, Layout};
use irq_safety::MutexIrqSafe; 
pub use slabmalloc::{ZoneAllocator, ObjectPage8k, AllocablePage};
use kernel_config::memory::PAGE_SIZE;
use core::ptr::NonNull;
use core::mem::transmute;

/// The maximum size that can be allocated through the allocator.
/// Any requests larger than this will be allocated directly from the OS.
pub const MAX_ALLOC_SIZE: usize = ZoneAllocator::MAX_ALLOC_SIZE;

/// The size in bytes of the Object Page used in this allocator.
/// Object Page is the unit of memory which the allocator works with to add and return memory.
pub const OBJECT_PAGE_SIZE_BYTES: usize = ObjectPage8k::SIZE;

/// The size in pages of the Object Page used in this allocator.
/// Object Page is the unit of memory which the allocator works with to add and return memory.
pub const OBJECT_PAGE_SIZE_PAGES: usize = ObjectPage8k::SIZE / PAGE_SIZE;

/// The number of sizes for which the allocator maintains separate slab allocators.
pub const MAX_SIZE_CLASSES: usize = ZoneAllocator::MAX_BASE_SIZE_CLASSES;

/// Creates a new ObjectPage given a virtual address
/// The page starting with the vaddr must be mapped and aligned to an 8k boundary!
pub unsafe fn create_object_page(vaddr: usize) -> Result< &'static mut ObjectPage8k<'static>, &'static str> {
    if vaddr % OBJECT_PAGE_SIZE_BYTES != 0 {
        error!("The object pages for the heap are not aligned at 8k bytes");
        return Err("The object pages for the heap are not aligned at 8k bytes");
    }
    Ok(transmute(vaddr)) 
}


/// This is mostly copied from LockedHeap, just to use IrqSafe versions instead of spin::Mutex.
pub struct IrqSafeHeap(MutexIrqSafe<ZoneAllocator<'static>>);

impl IrqSafeHeap {
    /// Creates an empty heap. All allocate calls will return `None`.
    pub const fn empty() -> IrqSafeHeap {
        IrqSafeHeap(MutexIrqSafe::new(ZoneAllocator::new()))
    }

    /// NOTE: the heap memory MUST BE MAPPED before calling this init function.
    /// The memory is divided evenly between the internal slab allocators.
    pub unsafe fn init(&self, start_virt_addr: usize, size_in_bytes: usize, heap_id: Option<usize>) -> Result<(), &'static str> {
        let num_object_pages = size_in_bytes / OBJECT_PAGE_SIZE_BYTES;
        let object_pages_per_slab = num_object_pages / MAX_SIZE_CLASSES;
        let sizes = &ZoneAllocator::BASE_ALLOC_SIZES;
        
        for slab in 0..MAX_SIZE_CLASSES {
            let slab_addr = start_virt_addr + (slab * object_pages_per_slab * OBJECT_PAGE_SIZE_BYTES); 
            for i in 0..object_pages_per_slab {
                // the starting address of the slab
                let addr = slab_addr + i*OBJECT_PAGE_SIZE_BYTES;

                // write the heap id to the end of the page
                let page = create_object_page(addr)?;
                if let Some(id) = heap_id {
                    page.heap_id = id;
                }

                // the alignment is equal to the size unless the size is not a multiple of 2
                let mut alignment = sizes[slab];
                if alignment == ZoneAllocator::MAX_BASE_ALLOC_SIZE {
                    alignment = 8;
                }

                let layout = Layout::from_size_align(sizes[slab], alignment).unwrap();
                // The page metadata has to be initalized to zero before refilling. If it's not then call page.clear_metadata() before.
                self.refill(layout, page)?; 
                trace!("Added an object page {:#X} to slab of size {}", addr, sizes[slab]);
            }
        }

        Ok(())
    }

    /// Adds a page to slab which allocates the given layout
    pub unsafe fn refill(&self, layout: Layout, page: &'static mut ObjectPage8k<'static>) -> Result<(), &'static str> {
        self.0.lock().refill(layout, page).map_err(|_e| "Heap_irq_safe:: unable to refill slab")
    }

    /// Returns an empty (unused) page if available
    pub unsafe fn return_page(&self) -> Option<&'static mut ObjectPage8k<'static>> {
        self.0.lock().return_page()
    }

    /// The total number of empty pages in the heap
    pub fn empty_pages(&self) -> usize {
        self.0.lock().empty_pages()
    }
}

unsafe impl GlobalAlloc for IrqSafeHeap {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut heap = self.0.lock(); 
        match heap.allocate(layout) {
            Ok(nptr) => nptr.as_ptr(),

            // The slab for the givern layout is OOM
            // We first try to see if another slab in the given heap has an empty page it can return
            Err(_x) => {
                warn!("Out of memory");
                let ptr = 
                    // there is an available page 
                    if let Some(page) = heap.return_page() {
                        page.clear_metadata();
                        let _ = heap.refill(layout, page); // if the refill fails then a null pointer will be returned again

                        match heap.allocate(layout) {
                            Ok(nptr) => {
                                trace!("Transferred page within a heap");
                                nptr.as_ptr()
                            },
                            Err(_x) => 0 as *mut u8,
                        }
                    }
                    // there was no available empty page within the heap
                    else {
                        0 as *mut u8    
                    };
                ptr        
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(nptr) = NonNull::new(ptr) {
            self.0
                .lock()
                .deallocate(nptr, layout)
                .expect("Couldn't deallocate");
        } else {
            // Nothing to do (don't dealloc null pointers).
        }
    }
}

// #[global_allocator]
// static ALLOCATOR: IrqSafeHeap = IrqSafeHeap::empty();

// /// NOTE: the heap memory MUST BE MAPPED before calling this init function.
// pub unsafe fn init(start_virt_addr: usize, size_in_bytes: usize) -> Result<(), &'static str> {
//     let _ = ALLOCATOR.init(start_virt_addr, size_in_bytes, None);

//     Ok(()) 
// }