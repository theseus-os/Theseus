#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


#[macro_use] extern crate alloc;
extern crate console;
extern crate rtc;

use alloc::{Vec, String};


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let now = rtc::read_rtc();
    if console::print_to_console(format!("{}!\n", now)).is_err() {
        return -1;
    }

    0
}
