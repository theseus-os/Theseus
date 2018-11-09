#![no_std]
#![feature(alloc)]
#[macro_use] extern crate terminal_print;

extern crate alloc;
extern crate task;
extern crate getopts;
extern crate vfs;

use alloc::{Vec, String};
use alloc::arc::Arc;
use alloc::string::ToString;
use getopts::Options;
use core::ops::Deref;
use vfs::Path;
use vfs::FSNode;


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
    
    if !matches.free.is_empty() {
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

        let path = Path::new(matches.free[0].to_string());
        // navigate to the filepath specified by first argument
        match path.get(&curr_wr) {
            Some(file_dir_enum) => {
                match file_dir_enum {
                    FSNode::Dir(dir) => curr_env.lock().set_wd(dir),
                    FSNode::File(_) => {
                        println!("can't cd into file");
                        return -1;
                    }
                }
            },
            None => println!("Directory does not exist")
        };
    }
    return 0;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";