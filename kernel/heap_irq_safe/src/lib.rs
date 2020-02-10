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

use core::ops::Deref;
use alloc::alloc::{GlobalAlloc, Layout};
use irq_safety::MutexIrqSafe; 
use block_allocator::FixedSizeBlockAllocator;

#[global_allocator]
static ALLOCATOR: IrqSafeHeap = IrqSafeHeap::empty();

/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
pub fn init(start_virt_addr: usize, size_in_bytes: usize) {
    unsafe {
        ALLOCATOR.lock().init(start_virt_addr, size_in_bytes);
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

unsafe impl GlobalAlloc for IrqSafeHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock()
            .allocate(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock()
            .deallocate(ptr, layout)
    }
}
