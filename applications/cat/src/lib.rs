#![no_std]
#[macro_use] extern crate app_io;
// #[macro_use] extern crate log;

#[macro_use] extern crate alloc;
extern crate task;
extern crate getopts;
extern crate path;
extern crate fs_node;
extern crate core2;

use core::str;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use getopts::Options;
use path::Path;
use fs_node::FileOrDir;


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
    if matches.opt_present("h") {
        print_usage(opts);
        return 0;
    }
    if matches.free.is_empty() {
        if let Err(e) = echo_from_stdin() {
            println!("{}", e);
            return -1;
        }
        return 0;
    }
    
    let Ok(cwd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        println!("failed to get current task");
        return -1;
    };
    let path = Path::new(matches.free[0].to_string());
    
    // navigate to the filepath specified by first argument
    match path.get(&cwd) {
        Some(file_dir_enum) => { 
            match file_dir_enum {
                FileOrDir::Dir(directory) => {
                    println!("{:?} is a directory, cannot 'cat' non-files.", directory.lock().get_name());
                    return -1;
                }
                FileOrDir::File(file) => {
                    let mut file_locked = file.lock();
                    let file_size = file_locked.len();
                    let mut string_slice_as_bytes = vec![0; file_size];
                    
                    let _num_bytes_read = match file_locked.read_at(&mut string_slice_as_bytes,0) {
                        Ok(num) => num,
                        Err(e) => {
                            println!("Failed to read {:?}, error {:?}", file_locked.get_name(), e);
                            return -1;
                        }
                    };
                    let read_string = match str::from_utf8(&string_slice_as_bytes) {
                        Ok(string_slice) => string_slice,
                        Err(utf8_err) => {
                            println!("File {:?} was not a printable UTF-8 text file: {}", file_locked.get_name(), utf8_err);
                            return -1;
                        }
                    };
                    println!("{}", read_string);
                }
            }
        },
        _ => {
            println!("Couldn't find file at path {}", path);
            return -1;
        }
    };
    0
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

fn echo_from_stdin() -> Result<(), &'static str> {
    let stdin = app_io::stdin()?;
    let stdout = app_io::stdout()?;
    let mut buf = [0u8; 256];

    // Read from stdin and print it back.
    loop {
        let cnt = stdin.read(&mut buf).or(Err("failed to perform read"))?;
        if cnt == 0 { break; }
        stdout.write_all(&buf[0..cnt])
            .or(Err("faileld to perform write_all"))?;
    }
    Ok(())
}

const USAGE: &str = "Usage: cat [file ...]
concatenate and print files";
