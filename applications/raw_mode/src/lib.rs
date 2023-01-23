//! An application to test raw mode.

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

pub fn main(_args: Vec<String>) -> isize {
    if let Err(e) = run() {
        log::error!("{}", e);
        return 1;
    }
    0
}

fn run() -> Result<(), &'static str> {
    let discipline = app_io::line_discipline()?;
    discipline.set_raw();

    let stdin = app_io::stdin()?;
    let stdout = app_io::stdout()?;
    let mut buf = [0u8];

    loop {
        let byte_num = stdin.read(&mut buf).or(Err("failed to invoke read"))?;
        if byte_num == 0 {
            break;
        }
        stdout
            .write_all(&buf)
            .or(Err("failed to invoke write_all"))?;
    }

    Ok(())
}
