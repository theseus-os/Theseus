#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate console;

extern crate task;
extern crate getopts;

use getopts::Options;
use alloc::{Vec, String};
use self::task::TASKLIST;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("a", "all", "print all processes in detail");

    let matches = match opts.parse(&args) {
        Ok(m) => { m }
        Err(_f) => { panic!("{}", _f) }
    };
    if matches.opt_present("h") {
        return print_usage(opts)
    }
    let mut process_string =  {
        if matches.opt_present("a") {
            String::from(format!("{0:<4} | {1:<10} | {2:<15} | {3:<10} | {4:<10} \n", 
            "ID", "CPU CORE", "PINNED CORE", "RUNSTATE", "NAME"))
        }
        else {
            String::from(format!("{0:<4} | {1:<10} \n", 
            "ID", "NAME"))
        }
    };

    use alloc::string::ToString;

    for process in TASKLIST.iter() {
        let id = process.0;
        let cpu_core = &process.1.read().running_on_cpu;
        let runstate = &process.1.read().runstate;
        let pinned_core = &process.1.read().pinned_core.map(|x| x.to_string()).unwrap_or(String::from("None"));
        let name = &process.1.read().name;
        if matches.opt_present("a") {
            process_string.push_str(&format!("{0:<4} | {1:<10} | {2:<15} | {3:<10?} | {4:<10} \n", id, cpu_core, pinned_core, runstate, name));
        }
        else {
            process_string.push_str(&format!("{0:<4} | {1:<10} \n", id, name));
        }
        
    }
    println!("{}", process_string);
    return 0;
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: ps [options]");
    println!("{}", opts.usage(&brief));
    return 0;
}