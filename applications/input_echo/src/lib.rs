#![no_std]


extern crate alloc;
extern crate application_io;
#[macro_use] extern crate terminal_print;

use alloc::vec::Vec;
use alloc::string::String;


#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    loop {

        let result = application_io::get_input_bytes();

        let data = match result {
            Ok(data) => data,
            Err(_) => return 1
        };

        if let Some(line) = data {
            let parsed_line = String::from_utf8_lossy(&line);
            println!("{}", parsed_line);
        }
    }
}
