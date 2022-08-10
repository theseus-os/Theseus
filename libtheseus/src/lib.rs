#![no_std]

pub extern crate core2;
pub extern crate panic_entry_inner;
pub extern crate app_io;
extern crate heap;

pub use panic_entry_inner as _;

pub use app_io as stdio;

pub mod alloc {
    use core::alloc::{GlobalAlloc, Layout};
    use heap::GLOBAL_ALLOCATOR;

    pub unsafe fn alloc(layout: Layout) -> *mut u8 {
        GLOBAL_ALLOCATOR.alloc(layout)
    }

    pub unsafe fn alloc_zeroed(layout: Layout) -> *mut u8 {
        GLOBAL_ALLOCATOR.alloc_zeroed(layout)
    }

    pub unsafe fn dealloc(ptr: *mut u8, layout: Layout) {
        GLOBAL_ALLOCATOR.dealloc(ptr, layout);
    }

    pub unsafe fn realloc(ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        GLOBAL_ALLOCATOR.realloc(ptr, layout, new_size)
    }
}