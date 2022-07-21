#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_env = taskref.get_env();
        println!("{}", curr_env.lock().cwd());
        0
    } else {
        println!("failed to get current task");    
        -1
    }
}