#![no_std]
#[macro_use] extern crate app_io;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    task::with_current_task(|t| {
        println!("{}", t.get_env().lock().cwd());
        0
    }).unwrap_or_else(|_| {
        println!("failed to get current task");
        -1
    })
}