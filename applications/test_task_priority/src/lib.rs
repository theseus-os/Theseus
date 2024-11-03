//! Test for schedulers supporting priorities i.e. epoch and priority
//! schedulers.
//!
//! The test ensures that tasks are run in order of priority for at least one
//! time slice.

#![no_std]

extern crate alloc;

use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

static CURRENT_PRIORITY: AtomicUsize = AtomicUsize::new(MAX_PRIORITY);

const MAX_PRIORITY: usize = 63;

fn worker(priority: usize) {
    // Add a bit of chaos.
    //
    // NOTE: When using the epoch scheduler, the test relies on the fact that the
    // worker runs in less than one time slice, and so we can't yield.
    #[cfg(priority_scheduler)]
    task::schedule();

    let previous = CURRENT_PRIORITY.fetch_sub(1, Ordering::Relaxed);
    assert_eq!(previous, priority);
}

fn spawner(_: ()) {
    if !task::scheduler::supports_priority() {
        log::warn!("scheduler does not support priorities");
        return;
    }

    let current_cpu = cpu::current_cpu();

    let priorities = priorities();
    let mut tasks = Vec::with_capacity(MAX_PRIORITY);

    // We hold preemption here so that when the scheduler next runs, all the worker
    // tasks are unblocked and in a random order on the run queue. Holding
    // preemption is sufficient as we pin the worker threads to the same core as
    // the spawner thread.
    let guard = preemption::hold_preemption();

    for priority in priorities {
        let task = spawn::new_task_builder(worker, priority)
            .pin_on_cpu(current_cpu)
            .block()
            .spawn()
            .unwrap();
        assert!(task::scheduler::set_priority(
            &task,
            priority.try_into().unwrap()
        ));
        tasks.push(task);
    }

    for task in tasks.iter() {
        task.unblock().unwrap();
    }

    drop(guard);

    for task in tasks {
        matches!(task.join().unwrap(), task::ExitValue::Completed(_));
    }
}

/// Returns a shuffled list of priorities.
fn priorities() -> Vec<usize> {
    let mut priorities = (0..=MAX_PRIORITY).collect::<Vec<_>>();

    let mut rng = fastrand::Rng::with_seed(random::next_u64());
    rng.shuffle(&mut priorities);

    priorities
}

pub fn main(_: Vec<String>) -> isize {
    let current_cpu = cpu::current_cpu();
    // The spawning thread must be pinned to the same CPU as the worker threads.
    let task = spawn::new_task_builder(spawner, ())
        .pin_on_cpu(current_cpu)
        .spawn()
        .unwrap();
    matches!(task.join().unwrap(), task::ExitValue::Completed(_));
    0
}
