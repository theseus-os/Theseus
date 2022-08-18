#![no_std]

pub extern crate app_io;
pub extern crate core2;
pub extern crate panic_entry_inner;

extern crate environment;
extern crate heap;
extern crate kernel_config;
extern crate memory;
extern crate scheduler;
extern crate spawn;
extern crate stack;
extern crate task as theseus_task;

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

pub mod mem {
    pub use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
    pub use memory::get_kernel_mmi_ref;
}

pub mod task {
    pub use environment::EnvIter;
    pub use scheduler::schedule as yield_now;
    pub use spawn::new_task_builder;
    pub use stack::alloc_stack_by_bytes;
    pub use theseus_task::{get_my_current_task, get_my_current_task_id, TaskRef};
}
