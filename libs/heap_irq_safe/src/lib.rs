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
#![feature(global_allocator)]

#![feature(alloc, allocator_api)]

#![no_std]

extern crate alloc;
extern crate linked_list_allocator;
extern crate irq_safety; 
extern crate spin;

use core::ops::Deref;
use alloc::allocator::{Alloc, Layout, AllocErr};
use linked_list_allocator::Heap;
use irq_safety::MutexIrqSafe; 


#[global_allocator]
static ALLOCATOR: IrqSafeHeap = IrqSafeHeap::empty();

/// NOTE: the heap memory MUST BE MAPPED before calling this init function.
pub fn init(start_virt_addr: usize, size_in_bytes: usize) {
    unsafe {
        ALLOCATOR.lock().init(start_virt_addr, size_in_bytes);
    }
}




/// This is mostly copied from LockedHeap, just to use IrqSafe versions instead of spin::Mutex.
pub struct IrqSafeHeap(MutexIrqSafe<Heap>);

impl IrqSafeHeap {
    /// Creates an empty heap. All allocate calls will return `None`.
    pub const fn empty() -> IrqSafeHeap {
        IrqSafeHeap(MutexIrqSafe::new(Heap::empty()))
    }
}

impl Deref for IrqSafeHeap {
    type Target = MutexIrqSafe<Heap>;

    fn deref(&self) -> &MutexIrqSafe<Heap> {
        &self.0
    }
}

unsafe impl<'a> Alloc for &'a IrqSafeHeap {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        self.0.lock().allocate_first_fit(layout)
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        self.0.lock().deallocate(ptr, layout)
    }
}