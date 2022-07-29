#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use terminal_print::{print, println};

pub fn main(_: Vec<String>) -> isize {
    println!("starting sleep 1");
    sleep::sleep(1);
    println!("sleep 1 complete");

    let guard = scheduler::disable_preemption();
    drop(guard);

    println!("starting sleep 2");
    sleep::sleep(1);
    println!("sleep 2 complete");

    let guard = scheduler::disable_preemption();
    println!("starting sleep 3 (this sleep should not end)");
    println!(
        "you should restart Theseus now, as preemption won't be reenabled when this task is killed"
    );
    sleep::sleep(1);
    println!("sleep 3 complete");
    drop(guard);

    -1
}
