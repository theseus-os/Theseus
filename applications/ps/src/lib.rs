#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
extern crate console;
extern crate task;
extern crate getopts;

use getopts::Options;
use alloc::{Vec, String};
use self::task::TASKLIST;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    let program = _args[0].clone();
    
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("a", "all", "print all processes in detail");

    let matches = match opts.parse(&_args) {
        Ok(m) => { m }
        Err(_f) => { panic!("{}", _f) }
    };
    if matches.opt_present("h") {
        return print_usage(opts)
    }
    if matches.opt_present("a") {
        return print_detailed_processes()
    }
    else {
        return print_processes()
    }
}

fn print_detailed_processes()-> isize {
    let mut all_process_string = String::from(format!("{0:<4} | {1:<10} | {2:<15} | {3:<15} | {4:<10} \n", 
            "ID", "CPU CORE", "NAME", "PINNED CORE", "RUNSTATE"));
    for cur_process in TASKLIST.iter() {
        //all_process_string.push_str(&format!("{} \n", *cur_process.1.read().deref()));
        let id = cur_process.0;
        let cpu_core = &cur_process.1.read().running_on_cpu;
        let runstate = &cur_process.1.read().runstate;
        let pinned_core = &cur_process.1.read().pinned_core;
        let name = &cur_process.1.read().name;
        all_process_string.push_str(&format!("{0:<4} | {1:<10} | {2:<15} | {3:?} | {4:<10?} \n", id, cpu_core, name, pinned_core, runstate));
    }
    if console::print_to_console(all_process_string).is_err(){
        return -1;
    }
    return 0;
}

fn print_processes() -> isize {
    let mut all_process_string = String::from(format!("{0:<4} | {1:<10} \n", 
            "ID", "NAME"));
    for cur_process in TASKLIST.iter() {
        let id = cur_process.0;
        let name = &cur_process.1.read().name;
        all_process_string.push_str(&format!("{0:<4} | {1:<10} \n", id, name));
    }
    if console::print_to_console(all_process_string).is_err(){
        return -1;
    }
    return 0;
}

fn print_usage(opts: Options) -> isize {
    let brief = format!("Usage: ps [options]");
    if console::print_to_console(format!("{}", opts.usage(&brief))).is_err(){
        return -1;
    }
    return 0;
}