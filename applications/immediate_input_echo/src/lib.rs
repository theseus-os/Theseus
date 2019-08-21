#![no_std]

extern crate alloc;
extern crate core_io;
extern crate stdio;
extern crate app_io;
#[macro_use] extern crate log;

use alloc::vec::Vec;
use alloc::string::String;
use core_io::{Read, Write};

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if let Err(e) = run() {
        error!("{}", e);
        return 1;
    }
    0
}

fn run() -> Result<(), &'static str> {
    app_io::request_stdin_instant_flush()?;
    let stdin = app_io::stdin()?;
    let stdout = app_io::stdout()?;
    let mut buf = [0u8];

    // Can lock them at the same time. Won't run into deadlock.
    let mut stdin_locked = stdin.lock();
    let mut stdout_locked = stdout.lock();

    let mut cnt = 0;
    while cnt < 10 {
        let byte_num = stdin_locked.read(&mut buf).or(Err("failed to invoke read"))?;
        if byte_num == 0 && stdin_locked.is_eof() { break; }
        else if byte_num == 0 { continue; }
        stdout_locked.write_all(&buf).or(Err("failed to invoke write_all"))?;
        cnt += 1;
    }
    stdout_locked.write_all(&['\n' as u8]).or(Err("failed to invoke write_all"))?;
    Ok(())
}
