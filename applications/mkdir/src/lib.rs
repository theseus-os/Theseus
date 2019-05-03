#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;
extern crate fs_node;
extern crate vfs_node;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::string::ToString;
// use fs_node::FileOrDir;
use vfs_node::VFSDirectory;

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
                    Arc::clone(&curr_env.working_dir)
                };
                let _new_dir = match VFSDirectory::new(dir_name.to_string(), &curr_dir) {
                    Ok(dir) => dir,
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