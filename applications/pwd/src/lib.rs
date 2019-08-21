#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_env = &taskref.lock().env;
        println!("{} \n", curr_env.lock().get_wd_path());
    } else {
        println!("failed to get task ref");    
    }
    0
}