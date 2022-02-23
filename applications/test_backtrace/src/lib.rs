#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate task;

extern crate backtrace;


use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;


pub fn main(_args: Vec<String>) -> isize {
    info!("test_backtrace::main(): at top");

    backtrace::trace(|frame| {
        println!("Frame: {:X?}", frame);
        true
    });
   
    0
}

