//! This scheduler implements a priority algorithm.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use log::error;
use runqueue_priority::RunQueue;
use task::TaskRef;

pub use runqueue_priority::{
    get_priority, inherit_priority, set_priority, PriorityInheritanceGuard,
};

/// This defines the priority scheduler policy.
/// Returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef> {
    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task_priority(): couldn't get runqueue for core {apic_id}",);
            return None;
        }
    };

    // This is a temporary solution before the PR to only store runnable tasks in
    // the run queue is merged.
    let mut blocked_tasks = Vec::with_capacity(2);
    while let Some(mut task) = runqueue_locked.pop() {
        if task.is_runnable() {
            for t in blocked_tasks {
                runqueue_locked.push(t)
            }
            task.last_ran = time::now::<time::Monotonic>();
            runqueue_locked.push(task.clone());
            return Some(task.task);
        } else {
            blocked_tasks.push(task);
        }
    }
    for task in blocked_tasks {
        runqueue_locked.push(task);
    }
    Some(runqueue_locked.idle_task().clone())
}
