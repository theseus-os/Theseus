//! This crate contains common types used for memory mapping. 

#![no_std]
#![feature(step_trait, step_trait_ext)]

#[macro_use] extern crate cfg_if;
extern crate kernel_config;
#[macro_use] extern crate derive_more;
extern crate zerocopy;
extern crate paste;


cfg_if! {
if #[cfg(target_arch="x86_64")] {

extern crate bit_field;
extern crate entryflags_x86_64;
extern crate multiboot2;
extern crate xmas_elf;
mod x86_64;
pub use x86_64::*;

}

else if #[cfg(target_arch="arm")] {

mod armv7em;
pub use armv7em::*;

}

}
