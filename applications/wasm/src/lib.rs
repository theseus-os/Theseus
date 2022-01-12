#![no_std]

#[macro_use]
extern crate alloc;
extern crate fs_node;
extern crate getopts;
#[macro_use]
extern crate terminal_print;
extern crate path;
extern crate task;
extern crate wasi_interpreter;

use alloc::{string::String, sync::Arc, vec::Vec};
use fs_node::FileOrDir;
use getopts::{Options, ParsingStyle};
use path::Path;

pub fn main(args: Vec<String>) -> isize {
    let mut opts = Options::new();
    opts.parsing_style(ParsingStyle::StopAtFirstFree);

    opts.optmulti("d", "dir", "set preopened directories", "DIR");
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
    let args: Vec<String> = matches.free;

    if args.is_empty() {
        print_usage(opts);
        return -1;
    }

    // get current working directory
    let curr_wr = Arc::clone(
        &task::get_my_current_task()
            .unwrap()
            .get_env()
            .lock()
            .working_dir,
    );

    // verify preopened directories are directories
    for dir in preopened_dirs.iter() {
        let dir_path = Path::new(dir.clone());

        match dir_path.get(&curr_wr) {
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

    let wasm_binary_path = Path::new(args[0].clone());

    // parse inputted wasm module into byte array
    let wasm_binary: Vec<u8> = match wasm_binary_path.get(&curr_wr) {
        Some(file_dir_enum) => match file_dir_enum {
            FileOrDir::Dir(directory) => {
                println!("{:?} is a directory.", directory.lock().get_name());
                return -1;
            }
            FileOrDir::File(file) => {
                let file_locked = file.lock();
                let file_size = file_locked.size();
                let mut string_slice_as_bytes = vec![0; file_size];

                let _num_bytes_read = match file_locked.read(&mut string_slice_as_bytes, 0) {
                    Ok(num) => num,
                    Err(e) => {
                        println!("Failed to read {:?}, error {:?}", file_locked.get_name(), e);
                        return -1;
                    }
                };
                string_slice_as_bytes
            }
        },
        _ => {
            println!("Couldn't find file at path '{}'", wasm_binary_path);
            return -1;
        }
    };

    // execute wasm binary
    wasi_interpreter::execute_binary(wasm_binary, args, preopened_dirs);

    0
}

fn print_usage(opts: Options) {
    println!("{}", opts.usage(USAGE));
}

const USAGE: &'static str = "USAGE:
    wasm </path/to/wasm/binary.wasm> [args ...]

EXAMPLES:

Running a WebAssembly Module:
    wasm example.wasm

Preopening Multiple Directories:
    wasm --dir DIR1 --dir DIR2 example.wasm

Passing Arguments to WebAssembly Module
    wasm example.wasm ARG1 ARG2 ARG3";
