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
    opts.optflag("b", "brief", "print only task id and name");

    let matches = match opts.parse(&args) {
        Ok(m) => { m }
        Err(_f) => { panic!("{}", _f) }
    };

    if matches.opt_present("h") {
        return print_usage(opts)
    }

    let mut process_string =  {
        if matches.opt_present("b") {
            String::from(format!("{0:5} | {1:10} \n", 
            "ID", "NAME"))
        }
        else {
            String::from(format!("{0:10} | {1:5} | {2:5} | {3:5} | {4:5} | {5:20} \n", 
            "RUNSTATE", "CPU", "PINNED", "TYPE", "ID", "NAME"))
        }
    };

    use alloc::string::ToString;

    for process in TASKLIST.iter() {
        let id = process.0;
        let name = &process.1.read().name;
        let runstate = &process.1.read().runstate;
        let cpu = &process.1.read().running_on_cpu;
        let pinned = &process.1.read().pinned_core.map(|x| x.to_string()).unwrap_or(String::from("None"));        
        let task_type = if process.1.read().is_an_idle_task {"I"}
                    else if process.1.read().app_crate.is_some() {"A"}
                    else {" "} ;     
        if matches.opt_present("b") {
            process_string.push_str(&format!("{0:5} | {1:10} \n", id, name));
        }
        else {
            process_string.push_str(&format!("{0:10?} | {1:5} | {2:5} | {3:5} | {4:5} | {5:20} \n", runstate, cpu, pinned, task_type, id, name));
        }
        
    }
    println!("{}", process_string);
    
    0
}

fn print_usage(opts: Options) -> isize {
    let mut brief = format!("Usage: ps [options] \n \n");

    brief.push_str("TYPE is 'I' if it is an idle task and 'A' if it is an application task. \n");
    brief.push_str("CPU is the cpu core the task is currently running on. \n");
    brief.push_str("PINNED is the core the task is pinned on, shows 'None' if not pinned to any. \n");
    brief.push_str("RUNSATE is runnability status of this task, i.e. whether it's allowed to be scheduled in. \n");
    brief.push_str("ID is the unique id of task. \n");
    brief.push_str("NAME is the simple name of the task");

    println!("{} \n", opts.usage(&brief));

    0
}