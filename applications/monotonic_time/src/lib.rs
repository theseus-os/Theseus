#![no_std]

extern crate alloc;

use alloc::{vec::Vec, string::String};
use terminal_print::println;

pub fn main(_args: Vec<String>) -> isize {
    let now = time::now::<time::Monotonic>();
    println!("{:#?}", now);
    0
}