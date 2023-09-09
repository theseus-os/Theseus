//! Application for testing the preemption counter (and, indirectly, CPU-local
//! variables).

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};

use app_io::println;
use preemption::{hold_preemption, preemption_enabled};

pub fn main(_: Vec<String>) -> isize {
    let guard = hold_preemption();

    if !guard.preemption_was_enabled() {
        println!("preemption was disabled by first guard");
        return -1;
    }

    if preemption_enabled() {
        println!("preemption was enabled after acquiring first guard");
        return -1;
    }

    let guard_2 = hold_preemption();

    if guard_2.preemption_was_enabled() {
        println!("preemption was disabled by second guard");
        return -1;
    }

    if preemption_enabled() {
        println!("preemption was enabled after acquring second guard");
        return -1;
    }

    drop(guard);

    if preemption_enabled() {
        println!("preemption was enabled after dropping first guard");
        return -1;
    }

    drop(guard_2);

    if !preemption_enabled() {
        println!("preemption was disabled after dropping second guard");
        return -1;
    }

    0
}
