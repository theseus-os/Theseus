//! A simple application to print the fault log

#![no_std]

extern crate alloc;
extern crate fault_log;

use alloc::{
    string::String,
    vec::Vec,
};

use fault_log::print_fault_log;

pub fn main(_args: Vec<String>) -> isize {
    print_fault_log();
    0
}
