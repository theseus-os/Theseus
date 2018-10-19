//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate terminal_print;
extern crate getopts;
extern crate ixgbe;

use alloc::{Vec, String};
use ixgbe::check_eicr;



#[no_mangle]
pub fn main(args: Vec<String>) -> isize{

    check_eicr();


    0
}

