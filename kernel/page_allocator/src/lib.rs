//! Provides an allocator for virtual memory pages.
//! The minimum unit of allocation is a single page. 
//! 
//! This also supports early allocation of pages (up to 32 separate chunks)
//! before heap allocation is available, and does so behind the scenes using the same single interface. 
//! 
//! Once heap allocation is available, it uses a dynamically-allocated list of page chunks to track allocations.
//! 
//! The core allocation function is [`allocate_pages_deferred()`](fn.allocate_pages_deferred.html), 
//! but there are several convenience functions that offer simpler interfaces for general usage. 
//!
//! # Notes and Missing Features
//! This allocator currently does **not** merge freed chunks (de-fragmentation). 
//! We don't need to do so until we actually run out of address space or until 
//! a requested address is in a chunk that needs to be merged;
//! that's where we should add those merging features in whenever we do so.

#![no_std]
#![feature(const_fn_trait_bound)]

#[macro_use] extern crate cfg_if;
extern crate kernel_config;
extern crate memory_structs;
extern crate alloc;
extern crate spin;
#[macro_use] extern crate static_assertions;
extern crate intrusive_collections;

cfg_if!{
if #[cfg(target_arch="x86_64")] {

#[macro_use] extern crate log;

mod x86_64;
pub use x86_64::*;

}

else if #[cfg(target_arch="arm")] {

#[macro_use] extern crate lazy_static;

mod armv7em;
pub use armv7em::*;

}
}
