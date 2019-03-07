//! This application is an example of how to write applications in Theseus.

#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate print;
extern crate getopts;
extern crate ixgbe;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;
use ixgbe::test_ixgbe_driver::test_nic_ixgbe_driver;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    test_nic_ixgbe_driver(None);  

    0
}
