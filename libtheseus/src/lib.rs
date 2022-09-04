#![no_std]

pub extern crate app_io;
pub extern crate core2;
pub extern crate panic_entry_inner;
pub extern crate path;

extern crate environment;
extern crate fs_node;
extern crate heap;
extern crate io;
extern crate kernel_config;
extern crate memory;
extern crate random;
extern crate scheduler;
extern crate spawn;
extern crate spin;
extern crate stack;
extern crate task as theseus_task;
extern crate thread_local_macro;

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

pub mod tls {
    pub use thread_local_macro::register_dtor;
}

pub mod mem {
    pub use kernel_config::memory::{KERNEL_STACK_SIZE_IN_PAGES, PAGE_SIZE};
    pub use memory::get_kernel_mmi_ref;
}

pub mod env {
    pub use environment::{EnvIter, Error};
}

pub mod task {
    pub use scheduler::schedule as yield_now;
    pub use spawn::new_task_builder;
    pub use stack::alloc_stack_by_bytes;
    pub use theseus_task::{get_my_current_task, get_my_current_task_id, JoinableTaskRef};
}

pub mod rand {
    pub use random::next_u64;
}

pub mod fs {
    // use fs_node::{File as FileTrait, FileRef};
    // use io::{KnownLength, LockableIo, ReaderWriter};
    // use spin::Mutex;

    // /// This is a typedef for a Theseus-native `FileRef` (`Arc<Mutex<dyn
    // /// File>>`) that is wrapped in a series of wrapper types, described
    // /// below from inner to outer.
    // ///
    // /// 1. The `FileRef` is wrapped in a `LockableIo` object
    // ///    in order to forward the various I/O traits (`ByteReader` +
    // /// `ByteWriter`)    through the `Arc<Mutex<_>>` wrappers.
    // /// 2. Then, that `LockableIo` <Arc<Mutex<File>>>` is wrapped in a
    // /// `ReaderWriter`    to provide standard "stateful" I/O that advances a
    // /// file offset. 3. Then, that `ReaderWriter` is wrapped in another
    // /// `Mutex` to provide    interior mutability, as the `Read` and `Write`
    // /// traits requires a mutable reference    (`&mut self`) but Rust
    // /// standard library allows you to call those methods on    an immutable
    // /// reference to its file, `&std::fs::File`. 4. That `Mutex` is then
    // /// wrapped in another `LockableIo` wrapper    to ensure that the IO
    // /// traits are forwarded, similar to step 1.
    // ///
    // /// In summary, the total type looks like this:
    // /// ```rust
    // /// LockableIo<Mutex<ReaderWriter<LockableIo<Arc<Mutex<dyn File>>>>>>
    // /// ```
    // ///
    // /// ... Then we take *that* and wrap it in an authentic parisian crepe
    // /// filled with egg, gruyere, merguez sausage, and portabello mushroom
    // /// ... [tacoooo townnnnn!!!!](https://www.youtube.com/watch?v=evUWersr7pc).
    // ///
    // /// TODO: redesign this to avoid the double Mutex. Options include:
    // /// * Change the Theseus `FileRef` type to always be wrapped by a
    // ///   `ReaderWriter`.
    // /// * Use a different wrapper for interior mutability, though Mutex is
    // ///   probably required.
    // /// * Devise another set of `Read` and `Write` traits that *don't* need
    // ///   `&mut self`.
    // pub type OpenFileRef = LockableIo<
    //     'static,
    //     ReaderWriter<LockableFileRef>,
    //     Mutex<ReaderWriter<LockableFileRef>>,
    //     Mutex<ReaderWriter<LockableFileRef>>,
    // >;

    // /// See the above documentation for [`OpenFileRef`].
    // pub type LockableFileRef =
    //     LockableIo<'static, dyn FileTrait + Send, Mutex<dyn FileTrait +
    // Send>, FileRef>;

    // pub use fs_node::DirRef;
}
