#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate task;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;
extern crate vfs;

use alloc::{Vec, String};
use vfs::Directory;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let mut tasks_string = String::from("");
        {
        // this is scoped so that we 
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        tasks_string.push_str(&curr_env.working_dir.lock().list_children());
        }
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        tasks_string.push_str(&curr_env.working_dir.lock().get_name());
        println!("running tasks:\n\n{}", tasks_string)
    }
    0
}