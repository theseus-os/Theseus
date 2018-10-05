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
use core::ops::Deref;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };
    
    if !matches.free.is_empty() {
        let taskref = match task::get_my_current_task() {
            Some(t) => t,
            None => {
                println!("failed to get current task");
                return -1;
            }
        };

        // navigate to the filepath specified by first argument
        let path: Vec<String> = matches.free[0].split("/").map(|s| s.to_string()).collect();
        let locked_task = taskref.lock();
        let mut curr_env = locked_task.env.lock();
        let mut new_wd = Arc::clone(&curr_env.working_dir);
        for dirname in path.iter() {
            // navigate to parent directory
            if dirname == ".." {
                let dir = match new_wd.lock().get_parent_dir() {
                    Some(dir) => dir,
                    None => {
                        print!("directory does not exist \n");
                        return -1;
                    }
                };
                new_wd = dir;
            }
            // navigate to child directory
            else {
                let dir = match new_wd.lock().get_child_dir(dirname.to_string()) {
                    Some(dir) => dir,
                    None => {
                        print!("directory does not exist \n");
                        return -1;
                    }
                };
                new_wd = dir;
            }
        }
        curr_env.set_wd(new_wd);
    }
    return 0;    
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";