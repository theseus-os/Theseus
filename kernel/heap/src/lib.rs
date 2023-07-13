//! The global allocator for the system. 
//! It starts off as a single fixed size allocator.
//! When a more complex heap is set up, it is set as the default allocator.

#![feature(allocator_api)]
#![no_std]

extern crate alloc;
extern crate sync_irq; 
extern crate spin;
extern crate memory;
extern crate kernel_config;
extern crate block_allocator;

use alloc::alloc::{GlobalAlloc, Layout};
use memory::PteFlags;
use kernel_config::memory::{KERNEL_HEAP_START, KERNEL_HEAP_INITIAL_SIZE};
use sync_irq::IrqSafeMutex;
use spin::Once;
use alloc::boxed::Box;
use block_allocator::FixedSizeBlockAllocator;


#[global_allocator]
pub static GLOBAL_ALLOCATOR: Heap = Heap::empty();

#[cfg(direct_access_to_multiple_heaps)]
/// The default allocator is the one which is set up after the basic system initialization is completed. 
/// Currently it is initialized with an instance of `MultipleHeaps`.
/// We only make the default allocator visible when we want to explicitly use it without going through the global allocator.
pub static DEFAULT_ALLOCATOR: Once<Box<dyn GlobalAlloc + Send + Sync>> = Once::new();

#[cfg(not(direct_access_to_multiple_heaps))]
/// The default allocator is the one which is set up after the basic system initialization is completed. 
/// Currently it is initialized with an instance of `MultipleHeaps`.
static DEFAULT_ALLOCATOR: Once<Box<dyn GlobalAlloc + Send + Sync>> = Once::new();

/// The heap mapped pages should be writable and non-executable.
pub const HEAP_FLAGS: PteFlags = PteFlags::from_bits_truncate(
    PteFlags::new().bits()
    | PteFlags::VALID.bits()
    | PteFlags::WRITABLE.bits()
);

/// The ending address of the initial heap. It is used to determine which heap should be used during deallocation.
const INITIAL_HEAP_END_ADDR: usize = KERNEL_HEAP_START + KERNEL_HEAP_INITIAL_SIZE;


/// Initializes the single heap, which is the first heap used by the system.
pub fn init_single_heap(start_virt_addr: usize, size_in_bytes: usize) {
    unsafe { GLOBAL_ALLOCATOR.initial_allocator.lock().init(start_virt_addr, size_in_bytes); }
}


/// Sets a new default allocator to be used by the global heap. It will start being used after this function is called.
pub fn set_allocator(allocator: Box<dyn GlobalAlloc + Send + Sync>) {
    DEFAULT_ALLOCATOR.call_once(|| allocator);
}


/// The heap which is used as a global allocator for the system.
/// It starts off with one basic fixed size allocator, the `initial allocator`. 
/// When a more complex heap is created and set as the `DEFAULT_ALLOCATOR`, then it is used.
pub struct Heap {
    initial_allocator: IrqSafeMutex<block_allocator::FixedSizeBlockAllocator>, 
}


impl Heap {
    /// Returns a heap in which only an empty initial allocator has been created.
    pub const fn empty() -> Heap {
        Heap {
            initial_allocator: IrqSafeMutex::new(FixedSizeBlockAllocator::new()),
        }
    }
}

unsafe impl GlobalAlloc for Heap {

    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        match DEFAULT_ALLOCATOR.get() {
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
            DEFAULT_ALLOCATOR.get()
                .expect("Ptr passed to dealloc is not within the initial allocator's range, and another allocator has not been set up")
                .dealloc(ptr, layout);
        }
    }

}
