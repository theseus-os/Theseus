#![no_std]


extern crate alloc;
#[macro_use] extern crate log;
#[macro_use] extern crate terminal_print;
extern crate task;

extern crate backtrace;


use alloc::vec::Vec;
use alloc::string::String;


pub fn main(_args: Vec<String>) -> isize {
    info!("test_backtrace::main(): at top");

    println!("Testing simple backtrace:");
    backtrace::trace(|frame| {
        println!("Frame: {:X?}", frame);
        true
    });

    println!("Testing resolved backtrace:");
    backtrace::trace(|frame| {
        println!("Frame: {:X?}", frame);
        backtrace::resolve_frame(frame, |symbol| println!("    Symbol: {:X?}", symbol));
        true
    });
   
    0
}

