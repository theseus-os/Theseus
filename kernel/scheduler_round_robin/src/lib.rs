#![no_std]
#![feature(alloc)]

extern crate alloc;
#[macro_use] extern crate log;
extern crate task;
extern crate runqueue;
extern crate runqueue_round_robin;

use task::TaskRef;
use runqueue_round_robin::RunQueue;



/// this defines the round robin scheduler policy.
/// returns None if there is no schedule-able task
pub fn select_next_task(apic_id: u8) -> Option<TaskRef>  {

    let mut runqueue_locked = match RunQueue::get_runqueue(apic_id) {
        Some(rq) => rq.write(),
        _ => {
            error!("BUG: select_next_task(): couldn't get runqueue for core {}", apic_id); 
            return None;
        }
    };
    
    let mut idle_task_index: Option<usize> = None;
    let mut chosen_task_index: Option<usize> = None;

    for (i, taskref) in runqueue_locked.iter().enumerate() {
        let t = taskref.lock();

        // we skip the idle task, and only choose it if no other tasks are runnable
        if t.is_an_idle_task {
            idle_task_index = Some(i);
            continue;
        }

        // must be runnable
        if !t.is_runnable() {
            continue;
        }

        // if this task is pinned, it must not be pinned to a different core
        if let Some(pinned) = t.pinned_core {
            if pinned != apic_id {
                // with per-core runqueues, this should never happen!
                error!("select_next_task() (AP {}) found a task pinned to a different core: {:?}", apic_id, *t);
                return None;
            }
        }
            
        // found a runnable task!
        chosen_task_index = Some(i);
        // debug!("select_next_task(): AP {} chose Task {:?}", apic_id, *t);
        break; 
    }

    // idle task is a backup iff no other task has been chosen
    chosen_task_index
        .or(idle_task_index)
        .and_then(|index| runqueue_locked.move_to_end(index))
}
