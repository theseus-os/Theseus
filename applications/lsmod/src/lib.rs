//! This application dumps out information about modules and crates in the system.

#![no_std]
#![feature(alloc)]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate getopts;
extern crate memory;
extern crate mod_mgmt;

use alloc::vec::Vec;
use alloc::string::String;
use getopts::Options;

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

    let namespace = match mod_mgmt::get_default_namespace() {
        Some(n) => n,
        _ => {
            println!("Error: unable to get default CrateNamespace"); 
            return -1;
        }
    };
    let mut out = String::new();

    if matches.opt_present("a") {
        out.push_str("==== Kernel Crate Files ====\n");
        for f in namespace.dirs().kernel_directory().lock().list() {
            out.push_str(&format!("{}\n", f)); 
        }
        out.push_str("\n==== Application Crate Files ====\n");
        for f in namespace.dirs().applications_directory().lock().list() {
            out.push_str(&format!("{}\n", f)); 
        }
    }
    else {
        for n in namespace.crate_names() {
            out.push_str(&format!("{}\t\t{:?}\n", n, namespace.get_crate(&n).map(|c| c.lock_as_ref().object_file_abs_path.clone())));
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
