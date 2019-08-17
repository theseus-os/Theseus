#![no_std]

extern crate alloc;
extern crate core_io;
extern crate scheduler;
extern crate shell;
extern crate app_io;
use core_io::Write;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;

#[no_mangle]
pub fn main(_args: Vec<String>) -> isize {
    if let Err(_) = run() { return 1; }
    return 0;
}

fn run() -> Result<(), &'static str> {
    let stdout = app_io::stdout()?;
    let queue = app_io::take_event_queue()?;
    let ack = "event received\n".to_string();
    let ack = ack.as_bytes();

    let mut cnt = 0;

    // Print an acknowledge message to the screen.
    // Note that one keyboard strike contains at least two events:
    // one pressing event and one releasing event.
    while cnt < 10 {
        if let Some(_key_event) = queue.read_one() {
            let mut stdout_locked = stdout.lock();
            stdout_locked.write_all(&ack).or(Err("bad write"))?;
            core::mem::drop(stdout_locked);
            cnt += 1;
        }
        scheduler::schedule();
    }

    Ok(())
}
