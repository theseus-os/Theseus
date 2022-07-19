#![no_std]

extern crate alloc;

use alloc::{vec::Vec, string::String};
use terminal_print::println;

pub fn main(_args: Vec<String>) -> isize {
    let secs = time::now::<time::Monotonic>().as_secs();
    let mins = secs / 60;
    let submin_secs = secs % 60;
    println!("{} min {} sec", mins, submin_secs);
    0
}