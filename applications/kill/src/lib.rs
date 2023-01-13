#![no_std]
extern crate alloc;
#[macro_use] extern crate app_io;

extern crate task;
extern crate runqueue;
extern crate getopts;

use getopts::Options;
use alloc::vec::Vec;
use alloc::string::String;

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

    println!("`kill` has temporarily been disabled because it needs to be reimplemented.");
    return -1;
}

#[allow(dead_code)]
fn print_usage(opts: Options) -> isize {
    let brief = "Usage: kill [OPTS] TASK_ID".to_string();
    println!("{}", opts.usage(&brief));
    0
}

