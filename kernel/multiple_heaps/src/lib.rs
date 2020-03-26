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
use irq_safety::MutexIrqSafe; 
use block_allocator::{HEADER_SIZE, FixedSizeBlockAllocator};
use raw_cpuid::CpuId;
use memory::{MappedPages, create_mapping, create_mapping_8k_aligned, EntryFlags, VirtualAddress};
use heap_irq_safe::{IrqSafeHeap, create_object_page, MAX_ALLOC_SIZE, OBJECT_PAGE_SIZE_BYTES, OBJECT_PAGE_SIZE_PAGES, ObjectPage8k, MAX_BASE_SIZE_CLASSES};
use kernel_config::memory::{PAGE_SIZE, MAX_HEAPS};

#[global_allocator]
static ALLOCATOR: MultipleHeaps = MultipleHeaps::empty();


/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
pub fn init(start_virt_addr: usize, size_in_bytes: usize) -> Result<(), &'static str>{
    let bytes_per_heap = size_in_bytes / MAX_HEAPS;

    if bytes_per_heap % PAGE_SIZE != 0 {
        return Err("Heap memory does not divide evenly on a page boundary");
    }

    for i in 0..MAX_HEAPS {
        unsafe {
            ALLOCATOR[i].init(start_virt_addr + i*bytes_per_heap, bytes_per_heap, Some(i))?
        }
    }

    Ok(())
}

pub const RETURN_THRESHOLD: usize = MAX_BASE_SIZE_CLASSES;

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
        self.iter().enumerate().max_by_key(|&(i, val)| val.empty_pages()).unwrap().0 //unwrap here since we know the heap is not empty
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

        if size > MAX_ALLOC_SIZE {
            let size_mp = core::mem::size_of::<MappedPages>();
            let mut ptr = 0 as *mut u8;

            if let Ok(mapped_pages) = create_mapping(size + size_mp, EntryFlags::WRITABLE) {
                ptr = mapped_pages.start_address().value() as *mut u8;
                let ptr_mp = ptr.offset(size as isize) as *mut MappedPages;
                ptr_mp.write(mapped_pages);
            }

            trace!("Allocated a large object of {} bytes", size);

            ptr

        }
        else {
            let heap_id = CpuId::new().get_feature_info().expect("Could not retrieve cpuid").initial_local_apic_id() as usize % MAX_HEAPS;
            let mut ptr = self[heap_id].alloc(layout);

            if ptr == (0 as *mut u8) {
                // first try to retrive an empty page from other heaps
                if let Some(page) = self.return_page() {
                    page.clear();
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
                else {
                    if let Some(mapped_pages) = allocate_8k_aligned_mapped_pages(OBJECT_PAGE_SIZE_PAGES) {
                        if let Ok(page) = create_object_page(mapped_pages.start_address().value()) {
                            match self[heap_id].refill(layout, page) {
                                Ok(()) => {
                                    trace!("Added an object page to the heap");
                                    ptr = self[heap_id].alloc(layout);
                                    
                                    // right now we forget any extra mapped pages added to the heap
                                    // TODO: rethink if this is the best way
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
        if size > MAX_ALLOC_SIZE {
            let ptr_mp = ptr.offset(size as isize) as *const MappedPages;
            unsafe { let mp = core::ptr::read(ptr_mp); }
            trace!("Deallocated a large object of {} bytes at address: {:#X}", size, ptr as usize);
            // ptr.write(155);
        }
        else {
            let page_addr = (ptr as usize) & !(OBJECT_PAGE_SIZE_BYTES - 1);
            let page = create_object_page(page_addr);
            assert!(page.is_ok()); 

            let mut heap_id = page.unwrap().heap_id;
            self[heap_id].dealloc(ptr, layout)

        }
    
        // trace!("deallocated to heap {}", heap_id);
    }
}
