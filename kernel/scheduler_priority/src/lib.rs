//! This scheduler implements a priority algorithm.
//!
//! Because the [`runqueue_priority::RunQueue`] internally sorts the tasks
//! in increasing order of periodicity, it's trivially easy to choose the next
//! task.

#![no_std]

extern crate alloc;

use log::error;
use runqueue_priority::RunQueue;
use task::TaskRef;

/// Set the periodicity of a given `Task` in all `RunQueue` structures.
/// A reexport of the set_periodicity function from runqueue_priority
pub use runqueue_priority::set_periodicity;

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

    if let Some((task_index, _)) = runqueue_locked
        .iter()
        .enumerate()
        .find(|(_, task)| task.is_runnable())
    {
        runqueue_locked.update_and_reinsert(task_index)
    } else {
        Some(runqueue_locked.idle_task().clone())
    }
}
