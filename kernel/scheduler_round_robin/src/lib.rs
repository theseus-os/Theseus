//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue.

#![no_std]

extern crate alloc;

use log::error;
use runqueue_round_robin::RunQueue;
use task::TaskRef;

/// This defines the round robin scheduler policy.
/// Returns None if there is no schedule-able task
// TODO: Remove option?
// TODO: Return &'static TaskRef?
pub fn select_next_task(apic_id: u8) -> Option<TaskRef> {
    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task_round_robin(): couldn't get runqueue for core {apic_id}",);
            return None;
        }
    };

    if let Some((task_index, _)) = runqueue_locked
        .iter()
        .enumerate()
        .find(|(_, task)| task.is_runnable())
    {
        runqueue_locked.move_to_end(task_index)
    } else {
        Some(runqueue_locked.idle_task().clone())
    }
}
