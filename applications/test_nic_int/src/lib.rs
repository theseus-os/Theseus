//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate ixgbe;

use alloc::{Vec, String};
use ixgbe::cause_interrupt;

static mut INT: u32 = 0;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize{
    
    unsafe{
        let x = 1 << INT;
        println!("Generating Interrupt: {:#X}", x);
        cause_interrupt(x);
        
        INT += 1;
        if INT == 15 { INT = 0;}
    }
    

    0
}

