#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate task;
#[macro_use] extern crate terminal_print;

use alloc::{Vec, String};

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_dir = taskref.read().get_wd();
        println!("{}", curr_dir.lock().list_children());
    }
    0
}
