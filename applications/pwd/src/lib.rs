#![no_std]
#[macro_use]
extern crate app_io;

extern crate alloc;
extern crate getopts;
extern crate task;

use alloc::{
    string::String,
    vec::Vec,
};

pub fn main(_args: Vec<String>) -> isize {
    task::with_current_task(|t| {
        println!("{}", t.get_env().lock().cwd());
        0
    })
    .unwrap_or_else(|_| {
        println!("failed to get current task");
        -1
    })
}
