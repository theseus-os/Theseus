//! A simple test crate for trying to build and use libc from Rust in Theseus.
//! 
//! This involves a ported version of the Rust libc crate that must be built in a way
//! that uses Theseus's `tlibc` as the underlying libc implementation. 

#![no_std]

use core::ptr;

use alloc::{vec::Vec, string::String};

extern crate alloc;
#[macro_use] extern crate log;
extern crate libc;


pub fn main(_args: Vec<String>) -> isize {
    test_mmap();
    test_printf();
    0
}

fn test_mmap() {
    warn!("Hello from test_mmap()");
    let ret = unsafe {
        libc::mmap(ptr::null_mut(), 100, 0, 0, 0, 0)
    };
    warn!("test_mmap: mmap returned {:?}", ret);
}


fn test_printf() {
    warn!("Hello from test_printf()");
    let ret = unsafe {
        libc::printf(b"%d".as_ptr() as *const i8, 17);
    };
    warn!("test_printf: printf returned {:?}", ret);
}

