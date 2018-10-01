#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::string::ToString;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_env = &taskref.lock().env;
        print!("{} \n", curr_env.lock().get_wd_path());
    } else {
        println!("failed to get task ref");    
    }
    0
}