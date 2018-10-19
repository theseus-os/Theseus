#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate task;
#[macro_use] extern crate terminal_print;
extern crate vfs;

use alloc::{Vec, String};
use vfs::Directory;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let locked_task = &taskref.lock();
        let curr_env = locked_task.env.lock();
        println!("{}", curr_env.working_dir.lock().list_children());
    }
    0
}