//! Sample test of a dynamically-linked library atop Theseus kernel crates.

#![no_std]
// #![no_main]
#![feature(start)]

extern crate alloc;
extern crate libtheseus;

use alloc::vec::Vec;
use alloc::string::String;


#[allow(dead_code)]
#[start]
fn start(_argc: isize, _argv: *const *const u8) -> isize {
    main();
    0
}

pub fn main() {
    let mut v = Vec::new();
    v.push(String::from("hi"));
    libtheseus::libtheseus_hello(v);
}

// this is obviously wrong, just a hack to get it to link
#[no_mangle]
extern "C" fn __libc_csu_init() { }
#[no_mangle]
extern "C" fn __libc_csu_fini() { }
#[no_mangle]
extern "C" fn __libc_start_main()  {}