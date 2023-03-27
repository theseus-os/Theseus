//! Application for running WASI-compliant WebAssembly binaries from Theseus command line.
//!
//! USAGE:
//!     wasm [option]... WASM_BINARY_PATH [arg]...
//!
//! EXAMPLES:
//!
//! Running a WebAssembly Binary:
//!     wasm example.wasm
//!
//! Preopening Multiple Directories:
//!     wasm --dir DIR1 --dir DIR2 example.wasm
//!
//! Passing Arguments/Flags to WebAssembly Binary:
//!     wasm --dir . example.wasm ARG1 ARG2 ARG3
//!

#![no_std]

#[macro_use]
extern crate alloc;
#[macro_use]
extern crate app_io;
extern crate fs_node;
extern crate getopts;
extern crate path;
extern crate task;
extern crate wasi_interpreter;

use alloc::{string::String, vec::Vec};
use fs_node::FileOrDir;
use getopts::{Options, ParsingStyle};
use path::Path;

pub fn main(args: Vec<String>) -> isize {
    // Parse command line options.
    let mut opts = Options::new();

    // StopAtFirstFree allows arguments to be passed to WebAssembly program.
    opts.parsing_style(ParsingStyle::StopAtFirstFree);

    opts.optmulti("d", "dir", "directories to grant file system access", "DIR");
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

    let preopened_dirs: Vec<String> = matches.opt_strs("d");

    // Get current working directory.
    let Ok(curr_wd) = task::with_current_task(|t| t.get_env().lock().working_dir.clone()) else {
        println!("failed to get current task");
        return -1;
    };

    // Verify passed preopened directories are real directories.
    for dir in preopened_dirs.iter() {
        let dir_path = Path::new(dir.clone());

        match dir_path.get(&curr_wd) {
            Some(file_dir_enum) => match file_dir_enum {
                FileOrDir::Dir(_) => {}
                FileOrDir::File(file) => {
                    println!("{:?} is a file.", file.lock().get_name());
                    return -1;
                }
            },
            _ => {
                println!("Couldn't find dir at path '{}'", dir_path);
                return -1;
            }
        };
    }

    let args: Vec<String> = matches.free;

    // Verify that arguments is non-empty.
    if args.is_empty() {
        println!("No WebAssembly path specified.");
        print_usage(opts);
        return -1;
    }

    let wasm_binary_path = Path::new(args[0].clone());

    // Parse inputted WebAssembly binary path into byte array.
    let wasm_binary: Vec<u8> = match wasm_binary_path.get(&curr_wd) {
        Some(file_dir_enum) => match file_dir_enum {
            FileOrDir::Dir(directory) => {
                println!("{:?} is a directory.", directory.lock().get_name());
                return -1;
            }
            FileOrDir::File(file) => {
                let mut file_locked = file.lock();
                let file_size = file_locked.len();
                let mut wasm_binary_as_bytes = vec![0; file_size];

                let _num_bytes_read = match file_locked.read_at(&mut wasm_binary_as_bytes, 0) {
                    Ok(num) => num,
                    Err(e) => {
                        println!("Failed to read {:?}, error {:?}", file_locked.get_name(), e);
                        return -1;
                    }
                };
                wasm_binary_as_bytes
            }
        },
        _ => {
            println!("Couldn't find file at path '{}'", wasm_binary_path);
            return -1;
        }
    };

    // Execute wasm binary.
    wasi_interpreter::execute_binary(wasm_binary, args, preopened_dirs);

    0
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &str = "USAGE:
    wasm [option]... WASM_BINARY_PATH [arg]...

EXAMPLES:

Running a WebAssembly Binary:
    wasm example.wasm

Preopening Multiple Directories:
    wasm --dir DIR1 --dir DIR2 example.wasm

Passing Arguments/Flags to WebAssembly Binary:
    wasm --dir . example.wasm ARG1 ARG2 ARG3";
