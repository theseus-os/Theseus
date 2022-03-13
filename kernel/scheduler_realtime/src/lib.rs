//! This crate picks the next task in accordance with the Rate Monotonic Scheduling algorithm,
//! a realtime scheduling algorithm that selects the task with the shortest period that is ready for execution.
//! We achieve this here by having the `RunQueue` structure internally sort the tasks in order of increasing periodicity.
//! Whenever a task is selected, it will be reinserted into the `RunQueue` at the same location if it is a periodic
//! task, or at the end if it is an aperiodic task.

#![no_std]

extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate runqueue;
extern crate runqueue_realtime;

use task::TaskRef;
use runqueue_realtime::RunQueue;

/// Set the periodicity of a given `Task` in all `RunQueue` structures.
/// A reexport of the set_periodicity function from runqueue_realtime
pub use runqueue_realtime::set_periodicity;

/// This defines the realtime scheduler policy.
/// Returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef> {
    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task_round_robin(): couldn't get runqueue for core {}", apic_id);
            return None;
        }
    };

    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;
    
    for (i, taskref) in runqueue_locked.iter().enumerate() {
        let t = taskref;

        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.is_an_idle_task {
            idle_task_index = Some(i);
            continue;
        }

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        // found a runnable task
        chosen_task_index = Some(i);
        break;
    }

    // idle task is backup iff no other task has been chosen
    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.update_and_reinsert(index))
}
