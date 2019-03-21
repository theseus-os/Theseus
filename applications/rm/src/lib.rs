#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;
// #[macro_use] extern crate log;

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
        Ok(exit_val) => {return exit_val;},
        Err(err) => {
            println!("{}", err);
            return -1;
        }
    }
}

pub fn remove_node(args: Vec<String>) -> Result<isize, &'static str> {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");
    
    let matches = match opts.parse(&args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return Ok(-1);
        }
    };

    let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            return Err("failed to get current task");
        }
    };
    // grabs the current environment pointer
    let curr_env = {
        let locked_task = taskref.lock();
        Arc::clone(&locked_task.env)
    };

    let curr_wr = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    let path = Path::new(matches.free[0].to_string());
    let delete_node: FileOrDir;
    // navigate to the filepath specified by first argument
    match path.get(&curr_wr) {
        Ok(node) => {
            delete_node = node;
        },
        Err(err) => {
            return Err(err); 
        }
    }; 

    if &delete_node.get_name() == root::ROOT_DIRECTORY_NAME {
        return Err("cannot remove root"); 
    }

    let parent = delete_node.get_parent_dir()?;
    parent.lock().delete_child(delete_node)?;
    Ok(0)
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: rm [PATH]
Remove node from filesystem";