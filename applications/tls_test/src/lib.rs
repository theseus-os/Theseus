#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate test_thread_local;

use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    println!("Invoking test_thread_local::test_tls()...");

    test_thread_local::test_tls(10);

    println!("Finished invoking test_thread_local::test_tls().");

    0
}
