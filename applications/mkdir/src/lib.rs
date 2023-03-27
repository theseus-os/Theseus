#![no_std]
#[macro_use] extern crate app_io;

extern crate alloc;
extern crate task;
extern crate getopts;
extern crate fs_node;
extern crate vfs_node;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
// use fs_node::FileOrDir;
use vfs_node::VFSDirectory;

pub fn main(args: Vec<String>) -> isize {
    if args.is_empty() {
        println!("Error: missing arguments");
        return -1;
    }

    let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        println!("failed to get current task");
        return -1;
    };

    let mut ret = 0;

    for dir_name in args.iter() {
        // add child dir to current directory
        if let Err(err) = VFSDirectory::create(dir_name.to_string(), &curr_wd) {
            println!("Error creating {:?}: {}", dir_name, err);
            ret = -1;
        }
    }

    ret
}