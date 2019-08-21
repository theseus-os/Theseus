#![no_std]

extern crate alloc;
extern crate core_io;
extern crate app_io;
#[macro_use] extern crate log;

use alloc::vec::Vec;
use alloc::string::String;
use core_io::Write;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if let Err(e) = run() {
        error!("{}", e);
        return 1;
    }
    0
}

fn run() -> Result<(), &'static str> {
    let mut stdin = app_io::stdin()?;
    let stdout = app_io::stdout()?;
    let mut stdout_locked = stdout.lock();

    let mut buf = String::new();

    // Read a line and print it back.
    loop {
        stdin.read_line(&mut buf).or(Err("failed to perform read_line"))?;
        stdout_locked.write_all(buf.as_bytes())
            .or(Err("faileld to perform write_all"))?;
        buf.clear();
    }
}
