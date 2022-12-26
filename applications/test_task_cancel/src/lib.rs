// TODO: Test that the other thread is succesfully cancelled in the following
// scenarios:
//
// 1. In lsda_generator, in which case it should trigger the first criteria of
// unwind::can_unwind.
//
// 2. At the call lsda_generator instruction, in which case it should trigger
// the second criteria of unwind::can_unwind.
//
// 3. At the jmp (loop) instruction, in which case it should continue to the
// next (call) instruction and then unwind.

#![no_std]

extern crate alloc;

use alloc::{string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use spin::Mutex;

pub fn main(_: Vec<String>) -> isize {
    let lock = Arc::new(Mutex::new(()));
    let task = spawn::new_task_builder(guard_hog, lock.clone())
        .spawn()
        .expect("failed to spawn task");

    while !lock.is_locked() {}

    task_cancel::cancel_task(task.clone());

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

    // Spend more time in lsda_generator to increase likelihood of scenario 1.
    static __COUNTER: AtomicUsize = AtomicUsize::new(0);
    __COUNTER.fetch_add(1, Relaxed);
}
