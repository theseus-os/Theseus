#![no_std]
#![feature(alloc)]

extern crate task;
#[macro_use] extern crate terminal_print;
#[macro_use] extern crate alloc;
#[macro_use] extern crate log;
extern crate fs_node;
extern crate getopts;
extern crate path;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use fs_node::FSNode;
use getopts::Options;
use path::Path;
use alloc::sync::Arc;

#[no_mangle]
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

    if matches.free.is_empty() {
        return 0;
    }

    let taskref = match task::get_my_current_task() {
        Some(t) => t,
        None => {
            println!("failed to get current task");
            return -1;
        }
    };

    // grabs the current environment pointer; this is scoped so that we drop the lock on the "cd" task
    let curr_env = {
        let locked_task = taskref.lock();
        Arc::clone(&locked_task.env)
    };

    // grabs the current working directory pointer; this is scoped so that we drop the lock on the "cd" task
    let curr_wd = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    let path = Path::new(matches.free[0].to_string());

    // navigate to the filepath specified by first argument
    match path.get(&curr_wd) {
        Ok(file_dir_enum) => {
            match file_dir_enum {
                FSNode::Dir(dir) => {
                    let mut child_string = String::new();
                    let mut child_list = dir.lock().list_children(); 
                    debug!("hello!? {}", dir.lock().get_name());   
                    child_list.reverse();
                    for child in child_list.iter() {
                        child_string.push_str(&format!("{}\n", child));
                    }
                    println!("{}", child_string);
                    return 0;
                },
                FSNode::File(file) => {
                    println!("'{}' is not a directory, cannot ls file", file.lock().get_name());
                    return -1;
                }
            }
        },
        Err(err) => {
            println!("get call in cd failed because: {}", err); 
            return -1;
        }
    };
    
    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";