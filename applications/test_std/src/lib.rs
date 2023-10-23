#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;

pub fn main(_: Vec<String>) -> isize {
    println!("starting");
    // unsafe { shim::hello() };
    println!("done");
    0
}
