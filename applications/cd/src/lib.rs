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
    
    if matches.free.is_empty() {
        // navigate to root directory
    } else {
        // navigate to the first directory passed in as arguments
        let target_dirname = &matches.free[0];
        let taskref = match task::get_my_current_task() {
            Some(t) => t,
            None => {
                println!("failed to get current task");
                return -1;
            }
        };
        let locked_task = taskref.lock();
        match locked_task.env.lock().set_chdir_as_wd(target_dirname.to_string()) {
                Ok(()) => {
                    return 0;
                }, 
                Err(_f) => {
                 println!("{}", _f);
                 return -1;
                }
            };
    }
    return -1;    
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";