#![no_std]

extern crate task;
#[macro_use] extern crate app_io;
extern crate alloc;
extern crate fs_node;
extern crate getopts;
extern crate path;

use alloc::{
    string::String,
    string::ToString,
    vec::Vec,
};
use core::fmt::Write;
use fs_node::{FileOrDir, DirRef};
use getopts::Options;
use path::Path;

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

    let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        println!("failed to get current task");
        return -1;
    };
    // print children of working directory if no child is specified
    if matches.free.is_empty() {
        print_children(&curr_wd);
        return 0;
    }

    let path = Path::new(matches.free[0].to_string());

    // Navigate to the path specified by first argument
    match path.get(&curr_wd) {
        Some(FileOrDir::Dir(dir)) => {
            print_children(&dir);
            0
        }
        Some(FileOrDir::File(file)) => {
            println!("'{}' is not a directory; `ls` currently only supports listing directory contents.", file.lock().get_name());
            -1
        }
        _ => {
            println!("Couldn't find path: {}", path); 
            -1
        }
    }
}

fn print_children(dir: &DirRef) {
    let mut child_string = String::new();
    let mut child_list = dir.lock().list(); 
    child_list.reverse();
    for child in child_list.iter() {
        writeln!(child_string, "{child}").expect("Failed to write child_string");
    }
    println!("{}", child_string);
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &str = "Usage: ls [DIR | FILE]
List the contents of the given directory or info about the given file.
If no arguments are provided, it lists the contents of the current directory.";