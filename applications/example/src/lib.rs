//! This application is an example of how to write applications in Theseus.

#![no_std]

extern crate alloc;
#[macro_use] extern crate app_io;
extern crate getopts;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;


pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1; 
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }

    println!("This is an example application.\nArguments: {:?}", args);

    0
}



fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "Usage: example [ARGS]
An example application that just echoes its arguments.";
