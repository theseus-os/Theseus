#![no_std]
#![feature(alloc)]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate console;

use alloc::{Vec, String};


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    // info!("Hello, world! (from hello application)");
    println!("Hello, world! Args: {:?}", _args);
    
    0
}
