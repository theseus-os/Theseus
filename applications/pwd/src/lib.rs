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
        let curr_dir = &taskref.lock().working_dir;
        print!("{} \n",curr_dir.lock().get_path());
    } else {
        println!("failed to get task ref");    
    }
    0
}