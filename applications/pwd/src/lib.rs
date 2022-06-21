#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    let curr_env = task::current_task().get_env();
    println!("{} \n", curr_env.lock().get_wd_path());
    0
}
