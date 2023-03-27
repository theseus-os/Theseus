#![no_std]
#[macro_use] extern crate app_io;
// #[macro_use] extern crate log;

#[macro_use] extern crate alloc;
extern crate task;
extern crate getopts;
extern crate path;
extern crate fs_node;
extern crate root;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use getopts::Options;
use path::Path;
use fs_node::{FsNode, FileOrDir};


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

    let Ok(working_dir) = task::with_current_task(|t|
        t.get_env().lock().working_dir.clone()
    ) else {
        return Err("failed to get current task".into());
    };

    if matches.free.is_empty() {
        return Err("rm: missing argument".into());
    }

    for path_string in &matches.free {
        let path = Path::new(path_string.clone());
        let node_to_delete = match path.get(&working_dir) {
            Some(node) => node,
            _ => return Err(format!("Couldn't find path {path}")),
        };

        // Only remove directories if the user specified "-r". 
        let can_remove_dirs = matches.opt_present("r");
        let path_error = || { format!("Couldn't remove {} from its parent directory.", &path) };
        let parent = node_to_delete.get_parent_dir().ok_or_else(path_error)?;

        match node_to_delete {
            FileOrDir::File(_) => {
                parent.lock().remove(&node_to_delete).ok_or_else(path_error)?;
            } 
            FileOrDir::Dir(_) => {
                if can_remove_dirs {
                    parent.lock().remove(&node_to_delete).ok_or_else(path_error)?;
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


const USAGE: &str = "Usage: rm [PATH]
Remove files or directories from filesystem";