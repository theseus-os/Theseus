#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;
extern crate vfs;

use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use vfs::{VFSDirectory, FSNode};

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    if !(args.is_empty()) {
        for dir_name in args.iter() {
            // add child dir to current directory
            if let Some(taskref) = task::get_my_current_task() {
                // grabs a pointer to the current working directory; this is scoped so that we drop the lock on the "mkdir" task as soon as we're finished
                let curr_dir = {
                    let locked_task = &taskref.lock();
                    let curr_env = locked_task.env.lock();
                    Arc::clone(&curr_env.working_dir)};
                let new_dir = VFSDirectory::new_dir(dir_name.to_string());
                match curr_dir.lock().add_fs_node(dir_name.to_string(), FSNode::Dir(new_dir)) {
                    Ok(()) => { },
                    Err(err) => {println!("{}", err);
                                return -1;}
                };
            } else {
                println!("failed to get task ref");    
            }
        }
    } 

    0
}