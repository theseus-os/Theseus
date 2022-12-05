//! An application to test directly accessing keyboard event.

#![no_std]

#[macro_use] extern crate alloc;
extern crate core2;
extern crate scheduler;
extern crate app_io;
extern crate keycodes_ascii;
#[macro_use] extern crate log;

use core2::io::Write;
use alloc::vec::Vec;
use alloc::string::String;
use keycodes_ascii::Keycode;

pub fn main(_args: Vec<String>) -> isize {
    if let Err(e) = run() {
        error!("{}", e);
        return 1;
    }
    0
}

fn run() -> Result<(), &'static str> {
    let stdout = app_io::stdout()?;
    let mut stdout_locked = stdout.lock();
    let queue = app_io::take_key_event_queue()?;
    let queue = (*queue).as_ref().ok_or("failed to get key event reader")?;
    let ack = "event received\n".as_bytes();

    stdout_locked.write_all(
            format!("{}\n{}\n{}\n\n",
                "Acknowledge upon receiving a keyboard event.",
                "Note that one keyboard strike contains at least one pressing event and one releasing event.",
                "Press `q` to exit."
            ).as_bytes()
        )
        .or(Err("failed to perform write_all"))?;

    // Print an acknowledge message to the screen.
    // Note that one keyboard strike contains at least two events:
    // one pressing event and one releasing event.
    loop {
        if let Some(key_event) = queue.read_one() {
            stdout_locked.write_all(&ack).or(Err("failed to perform write_all"))?;
            if key_event.keycode == Keycode::Q { break; }
        }
        scheduler::schedule();
    }

    Ok(())
}
