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
// extern crate memchr;
extern crate libm;
extern crate memfs;
extern crate cbitset;
extern crate rand;
extern crate task;
extern crate fs_node;

pub mod mman;
pub mod string;
pub mod types;
pub mod errno;
pub mod c_str;
pub mod file;
pub mod stdio;
pub mod stdlib;
pub mod ctype;
pub mod unistd;
pub mod fcntl;

use self::file::{MAX_FILE_DESCRIPTORS, FILE_DESCRIPTORS};

pub fn init_libc() {
    let mut file_descriptors = FILE_DESCRIPTORS.lock();
    // we don't use 0,1,2 because they're standard file descriptors
    for fd in 0..3 {
        file_descriptors[fd] = false;
    }
}