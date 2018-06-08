#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate console;

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
                        println!("Killed task {} \n", task_ref.read().deref());
                        0
                    }
                    else {
                        println!("{} was already exited \n", task_ref.read().deref());
                        -1
                    }
                }
                else {
                    println!("Task ID does not exist \n");
                    -1  
                }
            }, 
            _ => { 
                println!("Not a valid task ID \n"); 
                -1
            },
        };   
    }
    -1
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: kill TASK_ID");
    println!("{} \n", opts.usage(&brief));
    0
}

