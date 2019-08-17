#![no_std]

extern crate alloc;
extern crate core_io;
extern crate app_io;

use alloc::vec::Vec;
use alloc::string::String;
use core_io::Write;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    let mut stdin = match app_io::stdin() {
        Ok(stdin) => stdin,
        Err(_) => return 1
    };
    let stdout = match app_io::stdout() {
        Ok(stdout) => stdout,
        Err(_) => return 1
    };

    let mut buf = String::new();
    loop {
        if let Err(_) =  stdin.read_line(&mut buf) {
            return 1;
        }
        if let Err(_) = stdout.lock().write_all(buf.as_bytes()) {
            return 1;
        }
        buf.clear();
    }
}
