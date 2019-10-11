#![no_std]

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
use fs_node::{FileOrDir, DirRef};
use getopts::Options;
use path::Path;
use alloc::sync::Arc;

#[no_mangle]
pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.optflag("h", "help", "print this help menu");

    //dump some info about the this loaded app crate
    {
        let curr_task = task::get_my_current_task().unwrap();
        let t = curr_task.lock();
        let app_crate = t.app_crate.as_ref().unwrap();
        let krate = app_crate.lock_as_ref();
        trace!("============== Crate {} =================", krate.crate_name);
        for s in krate.sections.values() {
            trace!("   {:?}", &*s.lock());
        }
    }

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

    // grabs the current working directory pointer; this is scoped so that we drop the lock on the "cd" task
    let curr_wd = {
        let locked_task = taskref.lock();
        let curr_env = locked_task.env.lock();
        Arc::clone(&curr_env.working_dir)
    };
    
    // print children of working directory if no child is specified
    if matches.free.is_empty() {
        print_children(&curr_wd);
        return 0;
    }

    let path = Path::new(matches.free[0].to_string());

    // navigate to the filepath specified by first argument
    match path.get(&curr_wd) {
        Some(file_dir_enum) => {
            match file_dir_enum {
                FileOrDir::Dir(dir) => {
                    print_children(&dir);
                    return 0;
                },
                FileOrDir::File(file) => {
                    println!("'{}' is not a directory.", file.lock().get_name());
                    return -1;
                }
            }
        },
        _ => {
            println!("Couldn't find path: {}", path); 
            return -1;
        }
    };
}

fn print_children(dir: &DirRef) {
    let mut child_string = String::new();
    let locked_dir = dir.lock();
    println!("Got lock for directory");
    let mut child_list = locked_dir.list(); 
    child_list.reverse();
    for child in child_list.iter() {
        child_string.push_str(&format!("{}\n", child));
    }
    println!("{}", child_string);
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}


const USAGE: &'static str = "Usage: cd [ARGS]
Change directory";