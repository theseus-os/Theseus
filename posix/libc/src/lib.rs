//! libc implementation for Theseus

#![no_std]
#![allow(non_camel_case_types)]
#![feature(slice_internals)] //TODO: use rust memchr crate
#![feature(const_raw_ptr_deref)]
#![feature(core_intrinsics)]
#![feature(thread_local)]
// #![feature(alloc)]

#[macro_use] extern crate log;
extern crate alloc;
extern crate kernel_config;
extern crate hashbrown;
extern crate memory;
#[macro_use]extern crate lazy_static;
extern crate spin;
extern crate libm;
extern crate memfs;
extern crate cbitset;
extern crate rand;
extern crate task;
extern crate fs_node;
extern crate vfs_node;
extern crate root;
extern crate cstr_core;
extern crate libc;

#[doc(no_inline)]
pub use libc::*;
#[doc(no_inline)]
pub use cstr_core::*;

pub mod error;
pub mod fs;
pub mod mman;
pub mod stdio;
pub mod stdlib;
pub mod unistd;

use self::fs::{init_file_descriptors, create_libc_directory};
use alloc::string::String;
use core::ops::Deref;

pub fn init_libc() -> Result<(), &'static str> {
    create_libc_directory()?;
    init_file_descriptors()?;
    Ok(())
}