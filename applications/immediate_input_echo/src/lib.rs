//! An application to test instant stdin flush.
#![no_std]

#[macro_use] extern crate alloc;
extern crate core2;
extern crate stdio;
extern crate app_io;
#[macro_use] extern crate log;

use alloc::vec::Vec;
use alloc::string::String;
use core2::io::{Read, Write};

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

    stdout_locked.write_all(
            format!("{}\n{}\n\n",
                "Echo upon receiving a character input.",
                "Press `ctrl-D` to exit cleanly."
            ).as_bytes()
        )
        .or(Err("failed to perform write_all"))?;

    loop {
        let byte_num = stdin_locked.read(&mut buf).or(Err("failed to invoke read"))?;
        if byte_num == 0 { break; }
        stdout_locked.write_all(&buf).or(Err("failed to invoke write_all"))?;
    }
    stdout_locked.write_all(&['\n' as u8]).or(Err("failed to invoke write_all"))?;
    Ok(())
}
