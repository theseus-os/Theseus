#![no_std]

#[macro_use] extern crate cfg_if;
extern crate alloc;
extern crate kernel_config;
extern crate memory_structs;
extern crate memory;
extern crate page_allocator;

cfg_if!{
// Architecture dependent code for x86_64.
if #[cfg(target_arch="x86_64")] {

#[macro_use] extern crate log;

mod arch_x86_64;
pub use arch_x86_64::*;

}

else if #[cfg(target_arch="arm")] {

mod arch_armv7em;
pub use arch_armv7em::*;

}
}
