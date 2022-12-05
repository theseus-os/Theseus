//! An application to test stdin.
#![no_std]

#[macro_use] extern crate alloc;
extern crate core2;
extern crate app_io;
#[macro_use] extern crate log;

use alloc::vec::Vec;
use alloc::string::String;
use core2::io::Write;

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

    stdout_locked.write_all(
            format!("{}\n{}\n\n",
                "Echo upon receiving a new input line.",
                "Press `ctrl-D` to exit cleanly."
            ).as_bytes()
        )
        .or(Err("failed to perform write_all"))?;

    let mut buf = String::new();

    // Read a line and print it back.
    loop {
        let cnt = stdin.read_line(&mut buf).or(Err("failed to perform read_line"))?;
        if cnt == 0 { break; }
        stdout_locked.write_all(buf.as_bytes())
            .or(Err("faileld to perform write_all"))?;
        buf.clear();
    }
    Ok(())
}
