#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate log;

extern crate alloc;
extern crate task;
extern crate getopts;
extern crate path;
extern crate fs_node;
extern crate root;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::string::ToString;
use getopts::Options;
use path::Path;
use fs_node::{FsNode, FileOrDir};


#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    match remove_node(args) {
        Ok(_) => 0,
        Err(err) => {
            println!("{}", err);
            -1
        }
    }
}

pub fn remove_node(args: Vec<String>) -> Result<(), String> {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    opts.optflag("r", "recursive", "recursively remove directories and their contents");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(e) => {
            print_usage(opts);
            return Err(e.to_string());
        }
    };

    if matches.opt_present("h") {
        print_usage(opts);
        return Ok(());
    }


    let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            return Err("failed to get current task".into());
        }
    };

    let working_dir = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    if matches.free.is_empty() {
        return Err("rm: missing argument".into());
    }

    for path_string in &matches.free {
        let path = Path::new(path_string.clone());
        let node_to_delete = match path.get(&working_dir) {
            Ok(node) => node,
            Err(e) => return Err(e.into()),
        };

        // Only remove directories if the user specified "-r". 
        let mut can_remove_dirs = matches.opt_present("r");
        let parent = node_to_delete.get_parent_dir()?;

        match node_to_delete {
            FileOrDir::File(_) => {
                parent.lock().remove(&node_to_delete)?;
            } 
            FileOrDir::Dir(_) => {
                if can_remove_dirs {
                    parent.lock().remove(&node_to_delete)?;
                } else {
                    println!("Skipping the removal of directory '{}', try specifying the \"-r\" flag", 
                        node_to_delete.get_name());
                }
            }
        }
    }

    Ok(())
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: rm [PATH]
Remove files or directories from filesystem";