#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate task;
extern crate runqueue;
extern crate runqueue_priority;
extern crate getopts;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;
use runqueue::RunQueueTrait;
use runqueue_priority::RunQueue;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{} \n", _f);
            return -1; 
        }
    };
    
    if matches.opt_present("h") {
        return print_usage(opts);
    }

    for task_id_str in matches.free.iter() {
        match task_id_str.parse::<usize>(){
            Ok(task_id) => {
                if let Some(task_ref) = task::get_task(task_id) {
                    use core::ops::Deref;
                    if task_ref.kill(task::KillReason::Requested)
                        .and_then(|_| RunQueue::remove_task_from_all(task_ref))
                        .is_ok() 
                    {
                        println!("Killed task {} \n", task_ref.lock().deref());
                        0
                    }
                    else {
                        println!("Task {} was already exited.\n", task_id);
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
    0
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: kill TASK_ID");
    println!("{} \n", opts.usage(&brief));
    0
}

