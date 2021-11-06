#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]


extern crate alloc;
// #[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate thread_local_storage;

use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    println!("Invoking thread_local_storage::test_tls()...");

    thread_local_storage::test_tls(10);

    println!("Finished invoking thread_local_storage::test_tls().");

    0
}
