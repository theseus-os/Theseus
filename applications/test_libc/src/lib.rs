#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate libc;

use alloc::vec::Vec;
use alloc::string::String;
use libc::*;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    println!("testing libc!!!");

    unsafe {
        let a = mman::malloc(64) as *mut u8; 

        *a = 0xF;
        *a.offset(1) = 0xE;
        *a.offset(2) = 0xD;


        println!("a = {}, {}, {}", *a, *a.offset(1), *a.offset(2));
    }
     
    0
}
