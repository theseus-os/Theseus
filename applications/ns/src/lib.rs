//! This application allows querying about and interacting with namespaces in Theseus, 
//! specifically `CrateNamespace`s.

#![no_std]
#[macro_use] extern crate alloc;
#[macro_use] extern crate terminal_print;

extern crate getopts;
extern crate memory;
extern crate task;
extern crate mod_mgmt;

use core::{
    ops::Deref,
    fmt::Write,
};
use alloc::{
    vec::Vec,
    string::String,
};
use getopts::{Options, Matches};
use mod_mgmt::CrateNamespace;


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("r", "recursive", "include recursive namespaces");
    opts.optflag("f", "files", "lists crate object files available in this namespace rather than currently-loaded crates");

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

    match rmain(matches) {
        Ok(_) => 0,
        Err(e) => {
            println!("Error: {}", e);
            -1
        }
    }
}


fn rmain(matches: Matches) -> Result<(), String> {
    let curr_task = task::get_my_current_task().ok_or_else(|| format!("unable to get current task"))?;
    let namespace = curr_task.get_namespace();

    let recursive = matches.opt_present("r");
    let mut output = String::new();

    if matches.opt_present("f") {
        print_files(&mut output, 0, namespace.deref(), recursive)
            .map_err(|_e| String::from("String formatting error"))?;
    } else {
        print_crates(&mut output, 0, namespace.deref(), recursive)
            .map_err(|_e| String::from("String formatting error"))?;
    }

    println!("{}", output);
    Ok(())
}


fn print_files(output: &mut String, indent: usize, namespace: &CrateNamespace, recursive: bool) -> core::fmt::Result {
    writeln!(output, "\n{:indent$}{} CrateNamespace has crate object files:", "", namespace.name, indent = indent)?;
    for f in namespace.dir().lock().list() {
        writeln!(output, "{:indent$}{}", "", f, indent = (indent + 4))?;
    }

    if recursive {
        if let Some(r_ns) = namespace.recursive_namespace() {
            print_files(output, indent + 2, r_ns.deref(), recursive)?;
        }
    }

    Ok(())
}


fn print_crates(output: &mut String, indent: usize, namespace: &CrateNamespace, recursive: bool) -> core::fmt::Result {
    writeln!(output, "\n{:indent$}{} CrateNamespace has loaded crates:", "", namespace.name, indent = indent)?;
    // We do recursion manually here so we can separately print each recursive namespace.
    namespace.for_each_crate(false, |crate_name, crate_ref| {
        let _ = writeln!(output, "{:indent$}{}    {}", "", crate_name, crate_ref.lock_as_ref().object_file_abs_path, indent = (indent + 4));
        true
    });

    if recursive {
        if let Some(r_ns) = namespace.recursive_namespace() {
            print_crates(output, indent + 2, r_ns.deref(), recursive)?;
        }
    }

    Ok(())
}


fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "\nUsage: ns [OPTION]
Lists the crates that are loaded in the currently-active crate namespace.";
