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

use self::fs::{MAX_FILE_DESCRIPTORS, FILE_DESCRIPTORS, create_file};
use alloc::string::String;
use core::ops::Deref;

pub fn init_libc() {
    let mut descriptors = FILE_DESCRIPTORS.lock();
    // we don't use 0,1,2 because they're standard file descriptors
    descriptors.push(Some(create_file(&String::from("stdin"))));
    descriptors.push(Some(create_file(&String::from("stdout"))));
    descriptors.push(Some(create_file(&String::from("stderr"))));

    // initialize the empty table of file descriptors
    for _ in 3..MAX_FILE_DESCRIPTORS {
        descriptors.push(None);
    }
}