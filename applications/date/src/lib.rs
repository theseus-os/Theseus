#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]

extern crate alloc;
#[macro_use]
extern crate app_io;
extern crate rtc;

use alloc::{
    string::String,
    vec::Vec,
};

pub fn main(_args: Vec<String>) -> isize {
    let now = rtc::read_rtc();
    println!("{}", now);

    0
}
