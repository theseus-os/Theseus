#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate app_io;

use alloc::{string::String, vec::Vec};

pub fn main(_args: Vec<String>) -> isize {
    log::info!("Hello, world! Args: {:?}", _args);
    0
}
