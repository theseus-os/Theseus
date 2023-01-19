#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use preemption::hold_preemption;
use time::{now, Monotonic};

pub fn main(_: Vec<String>) -> isize {
    let mut tasks = Vec::with_capacity(4);
    for i in 0..4 {
        tasks.push(
            spawn::new_task_builder(worker, ())
                .pin_on_core(i)
                .block()
                .spawn()
                .expect("failed to spawn worker"),
        );
    }

    let start = now::<Monotonic>();
    for task in tasks.iter() {
        task.unblock().expect("failed to unblock task");
    }

    for task in tasks {
        // JoinableTaskRef::join is inlined so that we can yield if the worker hasn't
        // exited minimising the impact our task has on the worker tasks.
        // TODO: Call join directly once it is properly implemented.
        while !task.has_exited() {
            scheduler::schedule();
        }

        while task.is_running() {
            scheduler::schedule();
        }

        task.join().expect("failed to join task");
    }
    let end = now::<Monotonic>();

    println!("time: {:#?}", end - start);

    0
}

fn worker(_: ()) {
    let guard = hold_preemption();
    for _ in 0..(1 << 24) {
        let temp_guard = hold_preemption();
        drop(temp_guard);
    }
    drop(guard)
}
