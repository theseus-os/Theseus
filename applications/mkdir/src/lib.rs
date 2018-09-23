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
    if !(args.is_empty()) {
        for dir_name in args.iter() {
            // add child dir to current directory
            if let Some(taskref) = task::get_my_current_task() {
                let curr_dir = &taskref.lock().working_dir;
                curr_dir.lock().new_dir(dir_name.to_string(), Arc::downgrade(&curr_dir));
            } else {
                println!("failed to get task ref");    
            }
        }
    }

    0
}