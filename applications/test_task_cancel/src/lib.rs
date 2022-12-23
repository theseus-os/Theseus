// TODO: Properly implement Task::kill so the test passes.

#![no_std]

extern crate alloc;

use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

pub fn main(_: Vec<String>) -> isize {
    let lock = Arc::new(Mutex::new(()));
    let task = spawn::new_task_builder(other, lock.clone())
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
fn other(lock: Arc<Mutex<()>>) {
    let _guard = lock.lock();
    loop {
        // In order to properly unwind the task we need to reach an instruction that is
        // covered by the unwind tables. Rust generates an unwind row for all call
        // sites so by placing a call site in the loop we ensure that the task will
        // reach an instruction from which it can unwind when it is told to cancel.
        unwind_row_generator();
    }
}

#[inline(never)]
fn unwind_row_generator() {
    static __COUNTER: AtomicUsize = AtomicUsize::new(0);

    // Prevents unwind_row_generator from being optimised away.
    __COUNTER.fetch_add(1, Ordering::Relaxed);
}
