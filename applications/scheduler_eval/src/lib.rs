#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use app_io::println;
use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use time::{now, Monotonic};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub fn main(_: Vec<String>) -> isize {
    let mut tasks = Vec::with_capacity(32);
    for _ in 0..32 {
        tasks.push(
            spawn::new_task_builder(worker, ())
                .block()
                .spawn()
                .expect("failed to spawn task"),
        );
    }

    let start = now::<Monotonic>();
    for task in tasks.iter() {
        task.unblock().expect("failed to unblock task");
    }

    for task in tasks {
        task.join().expect("failed to join task");
    }
    let end = now::<Monotonic>();

    println!("time: {:#?}", end - start);

    0
}

fn worker(_: ()) {
    let counter = COUNTER.fetch_add(1, Relaxed);

    if counter > 10000 {
        return;
    } else {
        scheduler::schedule();
    }
}
