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
use fs_node::FileOrDir;


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
    let curr_wr = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };

    // go to root directory 
    if matches.free.is_empty() {
        curr_env.lock().working_dir = Arc::clone(root::get_root());
        return 0;
    }

    let path = Path::new(matches.free[0].to_string());
    
    // navigate to the filepath specified by first argument
    match path.get(&curr_wr) {
        Some(file_dir_enum) => {
            match file_dir_enum {
                FileOrDir::Dir(dir) => {
                    curr_env.lock().working_dir = dir;
                },
                FileOrDir::File(file) => {
                    println!("{:?} is not a directory.", file.lock().get_name());
                    return -1;
                }
            }
        },
        _ => {
            println!("Couldn't find directory {}", path); 
            return -1;
        }
    };
    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [PATH]
Change directory";