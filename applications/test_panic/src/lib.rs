#![no_std]
#![feature(alloc)]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate input_event_manager;
extern crate panic_info; 
extern crate task;


use alloc::{Vec, String};
use alloc::boxed::Box;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    info!("test_panic::main(): at top");

    let _ = task::set_my_panic_handler(Box::new(|info| {
        println!("Caught a panic: {}", info);
    }));

    info!("test_panic::main(): registered panic handler. Calling panic...");


    panic!("yo i'm testing a panic!!");
}


// use panic_info::PanicInfo;
// fn panic_handler(info: &PanicInfo) {
//     println!("Caught a panic: {}", info);
// }
