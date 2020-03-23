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

use core::ops::Deref;
use alloc::alloc::{GlobalAlloc, Layout};
use irq_safety::MutexIrqSafe; 
use block_allocator::{HEADER_SIZE, FixedSizeBlockAllocator};
use raw_cpuid::CpuId;

const MAX_HEAPS: usize = 8;

#[global_allocator]
static ALLOCATOR: MultipleHeaps = MultipleHeaps::empty();

/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
pub fn init(start_virt_addr: usize, size_in_bytes: usize) {
    let bytes_per_heap = size_in_bytes / MAX_HEAPS;

    for i in 0..MAX_HEAPS {
        unsafe {
            ALLOCATOR[i].lock().init(start_virt_addr + i*bytes_per_heap, bytes_per_heap);
        }
    }
}


/// This is mostly copied from LockedHeap, just to use IrqSafe versions instead of spin::Mutex.
pub struct IrqSafeHeap(MutexIrqSafe<FixedSizeBlockAllocator>);

impl IrqSafeHeap {
    /// Creates an empty heap. All allocate calls will return `None`.
    pub const fn empty() -> IrqSafeHeap {
        IrqSafeHeap(MutexIrqSafe::new(FixedSizeBlockAllocator::new()))
    }
}

impl Deref for IrqSafeHeap {
    type Target = MutexIrqSafe<FixedSizeBlockAllocator>;

    fn deref(&self) -> &MutexIrqSafe<FixedSizeBlockAllocator> {
        &self.0
    }
}

pub struct MultipleHeaps([IrqSafeHeap; MAX_HEAPS]);

impl MultipleHeaps {
    pub const fn empty() -> MultipleHeaps {
        MultipleHeaps([IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty(), IrqSafeHeap::empty()])
    }
}

impl Deref for MultipleHeaps {
    type Target = [IrqSafeHeap; MAX_HEAPS];

    fn deref(&self) -> &[IrqSafeHeap; MAX_HEAPS] {
        &self.0
    }
}

unsafe impl GlobalAlloc for MultipleHeaps {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let heap_id = CpuId::new().get_feature_info().expect("Could not retrieve cpuid").initial_local_apic_id() as usize % MAX_HEAPS;
        let ptr = self[heap_id].lock().allocate(layout);
        if ptr != (0 as *mut u8) {
            let ptr_header = ptr.offset(layout.size() as isize) as *mut usize;
            ptr_header.write(heap_id);
        }

        // trace!("allocated to heap {}", heap_id);

        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let heap_id =  *(ptr.offset(layout.size() as isize) as *mut usize);
        self[heap_id].lock()
            .deallocate(ptr, layout);

        // trace!("deallocated to heap {}", heap_id);

    }
}
