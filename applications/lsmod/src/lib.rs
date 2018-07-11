//! This application dumps out information about modules and crates in the system.

#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate print;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;

use alloc::{Vec, String};
use getopts::{Options, Matches};
use memory::{get_module, ModuleArea};


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("a", "all", "lists all available modules, not just loaded ones");


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

    let mut out = String::new();

    if matches.opt_present("a") {
        for m in memory::module_iterator() {
            out.push_str(&format!("{}, {:#X} -- {:#X} ({:#X} bytes)\n", 
                m.name(), m.start_address(), m.start_address() + m.size(), m.size())
            );
        }
    }
    else {
        for n in mod_mgmt::get_default_namespace().crate_names() {
            out.push_str(&format!("{}\n", n));
        }
    }
    println!("{}", out);
    0
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "\nUsage: lsmod [OPTION]
Lists the modules that are currently loaded in the default crate namespace.";
