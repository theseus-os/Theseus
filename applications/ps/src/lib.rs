#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
extern crate console;
extern crate task;

use alloc::{Vec, String};
use self::task::TASKLIST;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    
    let mut all_process_string = String::from(format!("{0: <4} | {1: <10} | {2:15} | {3: <10} \n", 
            "ID", "CPU CORE", "RUNSTATE", "NAME"));

    use core::ops::Deref;
    for cur_process in TASKLIST.iter() {
        // all_process_string.push_str(&format!("{} \n", *cur_process.1.read().deref()));
        let id = cur_process.0;
        let cpu_core = &cur_process.1.read().running_on_cpu;
        let runstate = &cur_process.1.read().runstate;
        let name = &cur_process.1.read().name;
        all_process_string.push_str(&format!("{0: <4} | {1:<10} | {2:15?} | {3: <10} \n", id, cpu_core, runstate, name));
    }
    if console::print_to_console(all_process_string).is_err(){
        return -1;
    }
    return 0;
}
