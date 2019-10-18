//! libc implementation for Theseus

#![no_std]
#![allow(non_camel_case_types)]
#![feature(slice_internals)] //TODO: use rust memchr crate
#![feature(const_raw_ptr_deref)]
#![feature(core_intrinsics)]

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

pub mod mman;
pub mod string;
pub mod types;
pub mod errno;
pub mod c_str;
pub mod fs;
pub mod stdio;
pub mod stdlib;
pub mod ctype;
pub mod unistd;
pub mod fcntl;

use self::fs::{init_file_descriptors};
use alloc::string::String;
use core::ops::Deref;

pub fn init_libc() {
    init_file_descriptors();
}