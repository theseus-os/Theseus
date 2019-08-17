#![no_std]

extern crate alloc;
extern crate core_io;
extern crate stdio;
extern crate app_io;

use alloc::vec::Vec;
use alloc::string::String;
use core_io::{Read, Write};
use core::mem;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {

    let stdin = match app_io::stdin() {
        Ok(stdin) => stdin,
        Err(_) => return 1
    };
    let stdout = match app_io::stdout() {
        Ok(stdout) => stdout,
        Err(_) => return 1
    };
    let mut buf = [0u8];

    loop {
        let mut stdin_locked = stdin.lock();
        match stdin_locked.read(&mut buf) {
            Ok(cnt) => {
                if cnt == 0 && stdin_locked.is_eof() { return 0; }
                else if cnt == 0 { continue; }
                // never lock stdin and stdout at the same time to avoid deadlock
                mem::drop(stdin_locked);
                let mut stdout_locked = stdout.lock();
                match stdout_locked.write_all(&buf) {
                    Ok(_cnt) => {},
                    Err(_) => return 1
                }
            },
            Err(_) => return 1
        };
    }
}
