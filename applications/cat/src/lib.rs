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
        // navigate to the filepath specified by first argument
        let locked_task = taskref.lock();
        let mut curr_env = locked_task.env.lock();
        let path = Path::new(matches.free[0].to_string());
        
        // let mut new_wd = Arc::clone(&curr_env.working_dir);
        let child_files = match path.get(&curr_env.working_dir) {
            Some(dir) => {
                dir.lock().get_children_files()
            },
            None => {println!("Directory does not exist");
                        return -1;}
        };

        for child_file in child_files.iter() {
            if child_file.lock().get_name() == path.components()[path.components().len()-1] {
                println!("{}", child_file.lock().read());
                return 0;
            }
        }

    }
    return -1;
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";