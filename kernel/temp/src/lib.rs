#![feature(thread_local, trivial_bounds)]
#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

#[cls::cpu_local]
pub static FOO: AtomicU32 = AtomicU32::new(0);

pub fn temp() -> u32 {
    FOO.store(3, Ordering::Relaxed);
    FOO.load(Ordering::Relaxed)
}
