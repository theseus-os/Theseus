#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
extern crate console;
extern crate task;
extern crate getopts;

use getopts::Options;
use alloc::{Vec, String};

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let mut opts = Options::new();
    
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&_args) {
        Ok(m) => { m }
        Err(_f) => { panic!("{}", _f) }
    };
    
    if matches.opt_present("h") {
        return print_usage(opts)
    }
    for task_id_str in matches.free.iter() {
        match task_id_str.parse::<usize>(){
            Ok(task_id) => {
                if let Some(task_ref) = task::get_task(task_id) {
                    use core::ops::Deref;
                    if task_ref.write().kill(task::KillReason::Requested) {
                        if console::print_to_console(String::from(format!("Killed task {} \n", task_ref.read().deref()))).is_ok() {
                            return 0;
                        }
                    }
                    else {
                        if console::print_to_console(String::from(format!("Failed to kill task {} \n", task_ref.read().deref()))).is_ok() {
                            return -1;
                        } 
                    }
                }
                else {
                    if console::print_to_console(String::from(format!("Not a valid task id \n"))).is_ok() {
                            return -1;
                        } 
                }
            }, 
            _ => { 
                if console::print_to_console(String::from("Not a valid task ID \n")).is_ok() {
                    return -1;
                }
            },
        };   
    }
    return -1; 
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: kill [task id]");
    if console::print_to_console(format!("{} \n", opts.usage(&brief))).is_err() {
        return -1;
    }
    0
}


// NEED TO FIX
// reads only first argument
// don't know how to deal with `print_to_console` right now using is_ok()
// not sure why running ps again the original killed task is still there may not remove it from TASKLIST?