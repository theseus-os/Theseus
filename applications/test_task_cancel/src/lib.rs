// TODO: Properly implement Task::kill so the test passes.

#![no_std]

extern crate alloc;

use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering::Relaxed};
use spin::Mutex;

pub fn main(_: Vec<String>) -> isize {
    let lock = Arc::new(Mutex::new(()));
    let task = spawn::new_task_builder(guard_hog, lock.clone())
        .spawn()
        .expect("failed to spawn task");

    while !lock.is_locked() {}

    task.kill(task::KillReason::Requested)
        .expect("failed to abort task");

    log::debug!("waiting for lock to be unlocked");

    // For us to acquire the lock, the drop handler of the other thread's guard must
    // have been invoked.
    let _ = lock.lock();

    0
}

#[inline(never)]
fn guard_hog(lock: Arc<Mutex<()>>) {
    let _guard = lock.lock();
    loop {
        // We cannot inline the load of FALSE into this function as then LLVM will
        // generate an unwind row that covers the entire function, but a call site table
        // that only covers the instructions associated with the panic, which we would
        // never reach.
        lsda_generator();
    }
}

#[inline(never)]
fn lsda_generator() {
    static FALSE: AtomicBool = AtomicBool::new(false);

    // We need to give LLVM false hope that lsda_generator may unwind. Otherwise,
    // LLVM will generate an unwind row, but no LSDA for guard_holder.
    //
    // The potential panic also prevents lsda_generator from being optimised away.
    if FALSE.load(Relaxed) {
        panic!();
    }
}
