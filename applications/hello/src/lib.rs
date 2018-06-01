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
    console::print_to_console(String::from("Hello, world from hello app!\n"));
    // println!("Hello world app args: {:?}", _args);

    0
}
