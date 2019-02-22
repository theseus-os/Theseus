#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate debugit;

extern crate task;
extern crate runqueue;
extern crate getopts;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;
use runqueue::RunQueue;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("r", "reap", 
        "reap the task (consume its exit value) in addition to killing it, removing it from the task list."
    );
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            return -1; 
        }
    };
    
    if matches.opt_present("h") {
        return print_usage(opts);
    }
    
    let reap = matches.opt_present("r");

    for task_id_str in matches.free.iter() {
        match task_id_str.parse::<usize>(){
            Ok(task_id) => {
                match kill_task(task_id, reap) {
                    Ok(_) => { }
                    Err(e) => {
                        println!("{}", e);
                        return -1;
                    }
                }
            }, 
            _ => { 
                println!("Invalid argument {}, not a valid task ID (usize)", task_id_str); 
                return -1;
            },
        };   
    }
    0
}


fn kill_task(task_id: usize, reap: bool) -> Result<(), String> {
    if let Some(task_ref) = task::get_task(task_id) {
        if task_ref.kill(task::KillReason::Requested)
            .and_then(|_| RunQueue::remove_task_from_all(&task_ref))
            .is_ok() 
        {
            println!("Killed task {}", &*task_ref.lock());
            if reap {
                match task_ref.take_exit_value() {
                    Some(exit_val) => { 
                        println!("Reaped task {}, got exit value {}", task_id, debugit!(exit_val));
                        Ok(())
                    }
                    _ => {
                        Err(format!("Failed to reap task {}", task_id))
                    }
                }
            } 
            else {
                // killed the task successfully, no reap request, so return success.
                Ok(())
            }
        }
        else {
            if reap {
                // if we failed to kill the task, but it was a reap request, then reap it anyway.
                match task_ref.take_exit_value() {
                    Some(exit_val) => { 
                        println!("Reaped task {}, got exit value {}", task_id, debugit!(exit_val));
                        Ok(())
                    }
                    _ => {
                        Err(format!("Failed to reap task {}", task_id))
                    }
                }
            }
            else {
                // failed to kill the task, no reap request, so return failure.
                Err(format!("Failed to kill task {}, it was already exited.", task_id))
            }
        }
    }
    else {
        Err(format!("Task ID {} does not exist", task_id))
    }
}


fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: kill [OPTS] TASK_ID");
    println!("{}", opts.usage(&brief));
    0
}

