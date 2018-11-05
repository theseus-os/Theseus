#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate task;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;
extern crate vfs;

use alloc::{Vec, String};
use vfs::Directory;
use alloc::arc::Arc;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        // this is scoped so that we drop the lock on the task as soon as we get the working directory pointer
        let curr_wr = {
            let locked_task = taskref.lock();
            let curr_env = locked_task.env.lock();
            Arc::clone(&curr_env.working_dir)
        };
        println!("{}", curr_wr.lock().list_children())
    }
    0
}