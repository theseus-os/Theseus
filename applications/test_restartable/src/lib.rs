#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]
#[macro_use] extern crate log;

extern crate alloc;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use spawn::new_restartable_task_builder;

fn restartable_loop(arg :usize) -> Result<(), &'static str> {
    debug!("Hi, I'm restartable function with arg {}", arg);
    if(arg > 3){
        panic!("paniced");
    }
    return Ok(()); 
} 
    
pub fn main(args: Vec<String>) -> isize {

    let mut arg_val = 0;
    match args[0].as_str() {
        "exit" => {
			arg_val = 1;
		}
        "panic" => {
			arg_val = 17;
		}
        _arg => {
            arg_val = 0;
        }
    }

    let taskref1  = new_restartable_task_builder(restartable_loop, arg_val)
        .name(String::from("restartable_loop"))
        .spawn()
        .expect("Couldn't start the restartable task"); 

    taskref1.join().expect("Task 1 join failed");

    // This loop is necessary to keep the application from being dropped
    loop {

    }

    0
}
