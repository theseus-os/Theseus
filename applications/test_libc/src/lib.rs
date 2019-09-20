#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate libc;

use alloc::vec::Vec;
use alloc::string::String;
use libc::{rmalloc};

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    println!("testing libc!!!");

    let a = rmalloc(64);

    unsafe{
        *a = 0xF;
        *a.offset(1) = 0xE;
        *a.offset(2) = 0xD;


        println!("a = {:#X}, {:#X}, {:#X}", *a, *a.offset(1), *a.offset(2));
    }
     
    0
}
