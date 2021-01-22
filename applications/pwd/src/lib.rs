#![no_std]
#[macro_use] extern crate terminal_print;

extern crate alloc;
use task;


use alloc::vec::Vec;
use alloc::string::String;

pub fn main(_args: Vec<String>) -> isize {
    if let Some(taskref) = task::get_my_current_task() {
        let curr_env = &taskref.lock().env;
        println!("{} \n", curr_env.lock().get_wd_path());
    } else {
        println!("failed to get task ref");    
    }
    0
}