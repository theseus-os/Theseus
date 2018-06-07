#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
#[macro_use] extern crate console;
extern crate rtc;

use alloc::{Vec, String};


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let now = rtc::read_rtc();
    println!("{}", now);

    0
}
