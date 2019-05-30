//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(abi_x86_interrupt)]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate interrupts;
extern crate x86_64;


use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use interrupts::*;
use x86_64::structures::idt::{LockedIdt, ExceptionStackFrame, HandlerFunc};

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    for i in 0..256 {
        println!("{}", i);
        let _ = register_msi_interrupt(random_handler);
    }

    let _ = deregister_interrupt(80, random_handler);
    let _ = register_interrupt(80, random_handler);
    let _ = register_interrupt(80, random_handler);

    0
}

extern "x86-interrupt" fn random_handler(_stack_frame: &mut ExceptionStackFrame) {

    eoi(None);
}
