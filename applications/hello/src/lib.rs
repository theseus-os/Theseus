#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate captain;

use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    let val = captain::LOADING_TIME.load(core::sync::atomic::Ordering::SeqCst);
    // info!("Hello, world! (from hello application)");
    println!("Hello, world! Args: {:?}", _args);
    println!("loading time {} ns", val);

    0
}
