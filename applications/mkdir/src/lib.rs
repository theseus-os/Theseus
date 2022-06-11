#![no_std]
#[macro_use]
extern crate terminal_print;

extern crate alloc;
extern crate fs_node;
extern crate getopts;
extern crate task;
extern crate vfs_node;

use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
// use fs_node::FileOrDir;
use vfs_node::VFSDirectory;

pub fn main(args: Vec<String>) -> isize {
    if !(args.is_empty()) {
        for dir_name in args.iter() {
            // add child dir to current directory
            if let Some(taskref) = task::get_my_current_task() {
                // grabs a pointer to the current working directory; this is scoped so that we drop the lock on the "mkdir" task as soon as we're finished
                let curr_dir = Arc::clone(&taskref.get_env().lock().working_dir);
                let _new_dir = match VFSDirectory::new(dir_name.to_string(), &curr_dir) {
                    Ok(dir) => dir,
                    Err(err) => {
                        println!("{}", err);
                        return -1;
                    }
                };
            } else {
                println!("failed to get task ref");
            }
        }
    }

    0
}
