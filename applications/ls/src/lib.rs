#![no_std]

extern crate task;
#[macro_use]
extern crate app_io;
extern crate alloc;
extern crate fs_node;
extern crate getopts;
extern crate path;

use alloc::{
    string::String,
    vec::Vec,
};
use core::fmt::Write;

use fs_node::{
    DirRef,
    FileOrDir,
};
use getopts::Options;
use path::Path;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("s", "size", "print the size of each file in directory");

    let matches = match opts.parse(args) {
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

    let size_option = matches.opt_present("s");

    let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        println!("failed to get current task");
        return -1;
    };

    // print children of working directory if no child is specified
    if matches.free.is_empty() {
        print_children(&curr_wd, size_option);
        return 0;
    }

    let path: &Path = matches.free[0].as_ref();

    // Navigate to the path specified by first argument
    match path.get(&curr_wd) {
        Some(FileOrDir::Dir(dir)) => {
            print_children(&dir, size_option);
            0
        }
        Some(FileOrDir::File(file)) => {
            println!(
                "'{}' is not a directory; `ls` currently only supports listing directory contents.",
                file.lock().get_name()
            );
            -1
        }
        _ => {
            println!("Couldn't find path: {}", path);
            -1
        }
    }
}

fn print_children(dir: &DirRef, print_size: bool) {
    let mut child_string = String::new();
    let mut child_list = dir.lock().list();
    child_list.reverse();
    for child in child_list.iter() {
        let child_path = dir.lock().get(child).expect("Failed to get child path");
        if print_size {
            match &child_path {
                FileOrDir::File(file_ref) => {
                    let file = file_ref.lock();
                    writeln!(child_string, "   {}    {}", file.len(), child).expect("Failed to write child_string");
                },
                FileOrDir::Dir(_) => {
                    writeln!(child_string, "   --    {}", child).expect("Failed to write child_string");
                },
            };
        } else {
            writeln!(child_string, "{}", child).expect("Failed to write child_string");
        }
    }
    println!("{}", child_string);
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: ls [DIR | FILE]
List the contents of the given directory or info about the given file.
If no arguments are provided, it lists the contents of the current directory.";
