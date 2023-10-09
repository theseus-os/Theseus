//! This crate picks the next task in round robin fashion.
//! Each time the task at the front of the queue is picked.
//! This task is then moved to the back of the queue.

#![no_std]
#![feature(core_intrinsics)]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{intrinsics::unlikely, sync::atomic::Ordering};

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
    fn next(&mut self, current_task: TaskRef) -> Option<TaskRef> {
        let mut contains_current = false;
        while let Some(task) = self.queue.pop_front() {
            // log::info!("popping task: {task:?}");
            if task == current_task {
                // log::info!("contains current");
                contains_current = true;
                continue;
            }
            if task.is_runnable() {
                self.add(task.clone());
                return Some(task);
            } else {
                log::info!("removing task: {task:?}");
                task.expose_is_on_run_queue()
                    .store(false, Ordering::Release);
                // This check prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks `is_on_run_queue` so we have to do the inverse.
                if unlikely(task.is_runnable()) {
                    log::error!("stinky");
                    self.add(task.clone());
                    return Some(task);
                }
            }
        }

        if !contains_current && !current_task.is_an_idle_task {
            log::info!("WTF: {current_task:?}");
        }

        if contains_current && current_task.is_runnable() {
            self.add(current_task);
            None
        } else if contains_current {
            log::info!("removing current: {current_task:?}");
            current_task
                .expose_is_on_run_queue()
                .store(false, Ordering::Release);
            // This check prevents an interleaving where `TaskRef::unblock` wouldn't add
            // the task back onto the run queue. `TaskRef::unblock` sets the run state and
            // then checks `is_on_run_queue` so we have to do the inverse.
            if unlikely(current_task.is_runnable()) {
                log::error!("stinky");
                self.add(current_task.clone());
                return Some(current_task);
            } else {
                Some(self.idle_task.clone())
            }
        } else {
            Some(self.idle_task.clone())
        }
    }

    fn busyness(&self) -> usize {
        self.queue.len()
    }

    fn add(&mut self, task: TaskRef) {
        task.expose_is_on_run_queue().store(true, Ordering::Release);
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
