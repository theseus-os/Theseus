#![no_std]
#![feature(alloc)]

extern crate alloc;
extern crate task;
#[macro_use] extern crate terminal_print;

use alloc::{Vec, String};
use alloc::arc::Arc;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    /* if !args.is_empty() {
        let task_ref = task::get_my_current_task_id();
        let curr_dir = task_ref.read().unwrap().get_wd();
        let target = args[0];
        if target == ".." {

        } else {
            curr_dir = curr_dir.lock();
            for child in curr_dir.child_dirs.iter() {
                if child_dir.name == target {
                    
                }
            }
        }
        
        
        

    } */
    

    0
}
