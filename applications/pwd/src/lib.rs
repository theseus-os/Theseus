#![no_std]
#[macro_use]
extern crate terminal_print;

extern crate alloc;
extern crate getopts;
extern crate task;

use alloc::{string::String, vec::Vec};

pub fn main(_args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_env = taskref.get_env();
        println!("{} \n", curr_env.lock().get_wd_path());
    } else {
        println!("failed to get task ref");
    }
    0
}
