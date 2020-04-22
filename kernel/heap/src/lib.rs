//! The global allocator for the system. 
//! It starts off as a single fixed size allocator.
//! When a more complex heap is set up, it is set as the default allocator.

#![feature(const_fn)]
#![feature(allocator_api)]
#![feature(cfg_doctest)]
#![no_std]

extern crate alloc;
extern crate irq_safety; 
extern crate spin;
extern crate memory;
extern crate kernel_config;
extern crate block_allocator;

use alloc::alloc::{GlobalAlloc, Layout};
use memory::EntryFlags;
use kernel_config::memory::{KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use irq_safety::MutexIrqSafe;
use spin::Once;
use alloc::boxed::Box;
use block_allocator::FixedSizeBlockAllocator;


#[global_allocator]
static ALLOCATOR: Heap = Heap::empty();

/// The heap mapped pages should be writable
pub const HEAP_FLAGS: EntryFlags = EntryFlags::WRITABLE;

/// The ending address of the initial heap. It is used to determine which heap should be used during deallocation.
const INITIAL_HEAP_END_ADDR: usize = KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE;


/// Initializes the single heap, which is the first heap used by the system.
pub fn init_single_heap(start_virt_addr: usize, size_in_bytes: usize) {
    unsafe { ALLOCATOR.initial_allocator.lock().init(start_virt_addr, size_in_bytes); }
}


/// Sets a new default allocator for the global heap. It will start being used after this function is called.
pub fn set_allocator(allocator: Box<dyn GlobalAlloc + Send + Sync>) {
    ALLOCATOR.set_allocator(allocator);
}


/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, the `initial allocator`. 
/// When a more complex heap is created it is set as the default allocator by initializing the `allocator` field.
pub struct Heap {
    initial_allocator: MutexIrqSafe<block_allocator::FixedSizeBlockAllocator>, 
    allocator: Once<Box<dyn GlobalAlloc + Send + Sync>>,
}


impl Heap {
    /// Returns a heap in which only the empty initial allocator has been created.
    pub const fn empty() -> Heap {
        Heap {
            initial_allocator: MutexIrqSafe::new(FixedSizeBlockAllocator::new()),
            allocator: Once::new(),
        }
    }

    fn set_allocator(&self, allocator: Box<dyn GlobalAlloc + Send + Sync>) {
        self.allocator.call_once(|| allocator);
    }
}

unsafe impl GlobalAlloc for Heap {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match self.allocator.try() {
            Some(allocator) => {
                allocator.alloc(layout)
            }
            None => {       
                self.initial_allocator.lock().allocate(layout)
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if (ptr as usize) < INITIAL_HEAP_END_ADDR {
            self.initial_allocator.lock().deallocate(ptr, layout);
        }
        else {
            self.allocator.try()
                .expect("Ptr passed to dealloc is not within the initial allocator's range, and another allocator has not been set up")
                .dealloc(ptr, layout);
        }
    }

}
