//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue.

#![no_std]
#![feature(core_intrinsics)]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::intrinsics::likely;

use task::TaskRef;

pub struct Scheduler {
    idle_task: TaskRef,
    queue: VecDeque<TaskRef>,
}

impl Scheduler {
    pub const fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            queue: VecDeque::new(),
        }
    }
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        let len = self.queue.len();
        let mut i = 0;

        while i < len {
            let task = self.queue.pop_front().unwrap();

            if task.is_runnable() {
                self.queue.push_back(task.clone());
                return task;
            } else if likely(!task.is_complete()) {
                self.queue.push_back(task);
            }
            i += 1;
        }

        self.idle_task.clone()
    }

    fn busyness(&self) -> usize {
        self.queue.len()
    }

    fn add(&mut self, task: TaskRef) {
        self.queue.push_back(task);
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        let mut task_index = None;
        for (i, t) in self.queue.iter().enumerate() {
            if t == task {
                task_index = Some(i);
                break;
            }
        }

        if let Some(task_index) = task_index {
            self.queue.remove(task_index);
            true
        } else {
            false
        }
    }

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler::PriorityScheduler> {
        None
    }

    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        Box::new(self.queue.drain(..))
    }

    fn tasks(&self) -> Vec<TaskRef> {
        self.queue.clone().into()
    }
}
