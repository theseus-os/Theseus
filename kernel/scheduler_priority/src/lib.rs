// //! This scheduler implements a priority algorithm.

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, collections::BinaryHeap, vec::Vec};
use core::cmp::Ordering;

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
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        // This is a temporary solution before the PR to only store runnable tasks in
        // the run queue is merged.
        let mut blocked_tasks = Vec::with_capacity(2);
        while let Some(mut task) = self.queue.pop() {
            if task.task.is_runnable() {
                for t in blocked_tasks {
                    self.queue.push(t)
                }
                task.last_ran = time::now::<time::Monotonic>();
                self.queue.push(task.clone());
                return task.task;
            } else {
                blocked_tasks.push(task);
            }
        }
        for task in blocked_tasks {
            self.queue.push(task);
        }
        self.idle_task.clone()
    }

    fn add(&mut self, task: TaskRef) {
        self.queue
            .push(PriorityTaskRef::new(task, DEFAULT_PRIORITY));
    }

    fn busyness(&self) -> usize {
        self.queue.len()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        let old_len = self.queue.len();
        self.queue
            .retain(|priority_task| priority_task.task != *task);
        let new_len = self.queue.len();
        // We should have at most removed one task from the run queue.
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
}

impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let previous_len = self.queue.len();
        self.queue.retain(|t| t.task != *task);

        if previous_len != self.queue.len() {
            // We should have at most removed one task from the run queue.
            debug_assert_eq!(self.queue.len() + 1, previous_len);
            self.queue.push(PriorityTaskRef {
                // TODO: Don't take reference?
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

    fn inherit_priority(
        &mut self,
        task: &TaskRef,
    ) -> task::scheduler::PriorityInheritanceGuard<'_> {
        todo!()
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
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.priority.cmp(&other.priority) {
            // Tasks that were ran longer ago should be prioritised.
            Ordering::Equal => Some(self.last_ran.cmp(&other.last_ran).reverse()),
            ordering => Some(ordering),
        }
    }
}

impl Ord for PriorityTaskRef {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}
