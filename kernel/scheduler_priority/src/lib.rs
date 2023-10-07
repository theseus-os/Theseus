//! This scheduler implements a priority algorithm.

#![no_std]
#![feature(core_intrinsics)]

extern crate alloc;

use alloc::{boxed::Box, collections::BinaryHeap, vec::Vec};
use core::{cmp, intrinsics::unlikely, sync::atomic};

use task::TaskRef;
use time::Instant;

const DEFAULT_PRIORITY: u8 = 0;

pub struct Scheduler {
    idle_task: TaskRef,
    queue: BinaryHeap<PriorityTaskRef>,
}

impl Scheduler {
    pub fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            queue: BinaryHeap::new(),
        }
    }

    fn add_priority_task(&mut self, task: PriorityTaskRef) {
        task.task
            .expose_is_on_run_queue()
            .store(true, atomic::Ordering::Release);
        self.queue.push(task);
    }
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        while let Some(task) = self.queue.pop() {
            if task.task.is_runnable() {
                self.add_priority_task(task.clone());
                return task.task;
            } else {
                task.task
                    .expose_is_on_run_queue()
                    .store(false, atomic::Ordering::Release);
                // This check prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks `is_on_run_queue` so we have to do the inverse.
                if unlikely(task.task.is_runnable()) {
                    self.add_priority_task(task.clone());
                    return task.task;
                }
            }
        }
        self.idle_task.clone()
    }

    fn add(&mut self, task: TaskRef) {
        self.add_priority_task(PriorityTaskRef::new(task, DEFAULT_PRIORITY));
    }

    fn busyness(&self) -> usize {
        self.queue.len()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        let old_len = self.queue.len();
        self.queue
            .retain(|priority_task| priority_task.task != *task);
        let new_len = self.queue.len();
        // We should have removed at most one task from the run queue.
        debug_assert!(
            old_len - new_len < 2,
            "difference between run queue lengths was: {}",
            old_len - new_len
        );
        new_len != old_len
    }

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler::PriorityScheduler> {
        Some(self)
    }

    fn drain(&mut self) -> alloc::boxed::Box<dyn Iterator<Item = TaskRef> + '_> {
        Box::new(self.queue.drain().map(|priority_task| priority_task.task))
    }

    fn tasks(&self) -> Vec<TaskRef> {
        self.queue
            .clone()
            .into_iter()
            .map(|priority_task| priority_task.task)
            .collect()
    }
}

impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let previous_len = self.queue.len();
        self.queue.retain(|t| t.task != *task);

        if previous_len != self.queue.len() {
            // We should have at most removed one task from the run queue.
            debug_assert_eq!(self.queue.len() + 1, previous_len);
            self.queue.push(PriorityTaskRef {
                task: task.clone(),
                priority,
                // Not technically correct, but this will be reset next time it is run.
                last_ran: Instant::ZERO,
            });
            true
        } else {
            false
        }
    }

    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        for priority_task in self.queue.iter() {
            if priority_task.task == *task {
                return Some(priority_task.priority);
            }
        }
        None
    }
}

#[derive(Clone, Debug, Eq)]
struct PriorityTaskRef {
    task: TaskRef,
    priority: u8,
    last_ran: Instant,
}

impl PriorityTaskRef {
    pub const fn new(task: TaskRef, priority: u8) -> Self {
        Self {
            task,
            priority,
            last_ran: Instant::ZERO,
        }
    }
}

impl PartialEq for PriorityTaskRef {
    fn eq(&self, other: &Self) -> bool {
        self.priority.eq(&other.priority) && self.last_ran.eq(&other.last_ran)
    }
}

impl PartialOrd for PriorityTaskRef {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        match self.priority.cmp(&other.priority) {
            // Tasks that were ran longer ago should be prioritised.
            cmp::Ordering::Equal => Some(self.last_ran.cmp(&other.last_ran).reverse()),
            ordering => Some(ordering),
        }
    }
}

impl Ord for PriorityTaskRef {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}
