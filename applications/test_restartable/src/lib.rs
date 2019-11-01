#![no_std]
// #![feature(plugin)]
// #![plugin(application_main_fn)]
#[macro_use] extern crate log;

extern crate alloc;
extern crate spawn;

use alloc::vec::Vec;
use alloc::string::String;
use spawn::KernelRestartableTaskBuilder;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    fn restartable_loop(arg :usize) -> Result<(), &'static str> {
        debug!("Hi, I'm restartable function with arg {}", arg);
        // if(arg > 3){
        //     panic!("paniced");
        // }
        return Ok(()); 
    } 

    let taskref1  = KernelRestartableTaskBuilder::new(restartable_loop, 5)
        .name(String::from("restartable_loop"))
        .spawn()
        .expect("Couldn't start the restartable task"); 

    taskref1.join().expect("Task 1 join failed");

    // This loop is necessary to keep the application from being dropped
    loop {

    }

    0
}
