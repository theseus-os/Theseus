//! This crate implements the virtual memory subsystem for Theseus,
//! which is fairly robust and provides a unification between 
//! arbitrarily mapped sections of memory and Rust's lifetime system. 
//! Originally based on Phil Opp's blog_os. 

#![no_std]
#![feature(ptr_internals)]
#![feature(unboxed_closures)]

#[macro_use] extern crate cfg_if;
extern crate spin;
extern crate alloc;
extern crate zerocopy;
extern crate memory_structs;
extern crate page_allocator;

cfg_if!{
// Architecture dependent code for x86_64.
if #[cfg(target_arch="x86_64")] {

extern crate multiboot2;
#[macro_use] extern crate log;
extern crate irq_safety;
extern crate kernel_config;
extern crate atomic_linked_list;
extern crate xmas_elf;
extern crate bit_field;
#[cfg(target_arch = "x86_64")]
extern crate memory_x86_64;
extern crate x86_64;

mod x86_64_memory;
pub use x86_64_memory::*;

}

else if #[cfg(target_arch="arm")] {

mod armv7em_memory;
pub use armv7em_memory::*;

}
}
