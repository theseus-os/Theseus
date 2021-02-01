//! This crate contains common types used for memory mapping. 

#![no_std]
#![feature(const_fn)]
#![feature(step_trait, step_trait_ext)]

extern crate kernel_config;
#[macro_use] extern crate derive_more;
extern crate zerocopy;

cfg_if::cfg_if! {
if #[cfg(target_arch="x86_64")] {

extern crate bit_field;
extern crate entryflags_x86_64;
mod x86_64;
pub use x86_64::*;

}

else if #[cfg(target_arch="arm")] {

mod armv7em;
pub use armv7em::*;

}

}
