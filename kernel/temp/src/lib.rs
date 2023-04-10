#![feature(thread_local)]
#![no_std]

use core::cell::Cell;

#[cls::cpu_local]
pub static FOO: Cell<u32> = Cell::new(0);

pub fn temp() -> u32 {
    FOO.set(3);
    FOO.get()
}