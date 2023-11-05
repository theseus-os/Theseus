#![no_std]
#[macro_use] extern crate app_io;
// #[macro_use] extern crate log;

extern crate alloc;
extern crate fs_node;
extern crate getopts;
extern crate path;
extern crate root;
extern crate task;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use getopts::Options;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(args) {
        Ok(m) => m,
        Err(_f) => {
            println!("{}", _f);
            print_usage(opts);
            return -1;
        }
    };

    let Ok(curr_env) = task::with_current_task(|t| t.get_env()) else {
        println!("failed to get current task");
        return -1;
    };

    // go to root directory
    if matches.free.is_empty() {
        curr_env.lock().working_dir = Arc::clone(root::get_root());
    } else {
        let path = matches.free[0].as_ref();
        match curr_env.lock().chdir(path) {
            Err(environment::Error::NotADirectory) => {
                println!("not a directory: {}", path);
                return -1;
            }
            Err(environment::Error::NotFound) => {
                println!("couldn't find directory: {}", path);
                return -1;
            }
            _ => {}
        }
    }
    0
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "Usage: cd [PATH]
Change directory";
