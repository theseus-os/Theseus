//! This application tests the restartable tasks in the presence
//! of graceful exit, panic and exceptions.

#![no_std]

#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate alloc;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use spawn::new_task_builder;

#[derive(Clone)]
enum ExitMethod {
    Graceful,
    Panic,
    Exception,
}

fn restartable_loop(exit_method: ExitMethod) -> Result<(), &'static str> {
    match exit_method {
        ExitMethod::Graceful => {
            debug!("Hi, I'm restartable function");
        },
        ExitMethod::Panic => {
            debug!("Hi, I'm restartable function with panic");
            panic!("paniced");
        },
        ExitMethod::Exception => {
            debug!("Hi, I'm restartable function with exception");
            #[cfg(unwind_exceptions)]{
                debug!("Will not restart as it is compiled without unwind_exceptions directive");
            }
            // causes a page fault
            unsafe {*(0x5050DEADBEEF as *mut usize) = 0x5555_5555_5555;}
        },
    }

    return Ok(()); 
} 
    
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("p", "panic", "induce panic to restartable task");
    opts.optflag("x", "exception", "induce exception to restartable task");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    let mut exit_method = ExitMethod::Graceful;

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    if matches.opt_present("p") {
        exit_method = ExitMethod::Panic;
    }

    if matches.opt_present("x") {
        exit_method = ExitMethod::Exception;
    }

    let taskref1  = new_task_builder(restartable_loop, exit_method)
        .name(String::from("restartable_loop"))
        .restartable_spawn()
        .expect("Couldn't start the restartable task"); 

    taskref1.join().expect("Task 1 join failed");

    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "Usage: test_restartable [OPTION]
Spawns a simple restartable task that can encounter panic and exceptions.";