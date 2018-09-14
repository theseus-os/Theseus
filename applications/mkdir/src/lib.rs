#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;

use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use getopts::Options;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    // TODO - panics when no argument passed
    let mut opts = Options::new();
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{} \n", _f);
            return -1; 
        }
    };
    for dir_name in matches.free.iter() {
        // add child dir to current directory
        if let Some(taskref) = task::get_my_current_task() {
            let curr_dir = taskref.write().get_wd();
            curr_dir.lock().new_dir(dir_name.to_string(), Arc::downgrade(&curr_dir));
        } else {
            println!("failed to get task ref");
            
    }
    }

    
    0
}
