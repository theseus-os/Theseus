//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue.

#![no_std]

extern crate alloc;

use core::marker::PhantomData;

use log::error;
use runqueue_round_robin::RunQueue;
use task::TaskRef;

/// This defines the round robin scheduler policy.
/// Returns None if there is no schedule-able task
// TODO: Remove option?
// TODO: Return &'static TaskRef?
pub fn select_next_task() -> Option<TaskRef> {
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

struct RoundRobinScheduler {
    idle_task: TaskRef,
    queue: VecDeque<RoundRobinTaskRef>,
}

impl task::scheduler_2::Scheduler for RoundRobinScheduler {
    fn next(&mut self) -> TaskRef {
        if let Some((task_index, _)) = self
            .queue
            .iter()
            .enumerate()
            .find(|(_, task)| task.is_runnable())
        {
            let task = self.queue.swap_remove_front(task_index);
            self.queue.push_back(task.clone());
            task
        } else {
            self.idle_task.clone()
        }
    }

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler_2::PriorityScheduler> {
        None
    }
}
