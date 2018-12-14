#![no_std]
#![feature(alloc)]

extern crate task;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate fs_node;

use alloc::{Vec, String};
use fs_node::Directory;
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
        let mut child_string = String::new();
        let mut child_list = curr_wr.lock().list_children();
        child_list.reverse();
        for child in child_list.iter() {
            child_string.push_str(&format!("{}\n", child));
        }
        println!("{}",child_string);
    }
    0
}