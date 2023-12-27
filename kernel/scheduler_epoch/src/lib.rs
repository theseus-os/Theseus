//! A token-based epoch scheduler.
//!
//! The implementation is based on the [`O(1)` Linux
//! scheduler][linux-scheduler].
//!
//! [linux-scheduler]: https://litux.nl/mirror/kerneldevelopment/0672327201/ch04lev1sec2.html

#![no_std]
#![feature(core_intrinsics)]

mod queue;

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use core::{
    mem,
    ops::{Deref, DerefMut},
};

use task::TaskRef;

use crate::queue::RunQueue;

const MAX_PRIORITY: u8 = 63;
const DEFAULT_PRIORITY: u8 = 20;
const INITIAL_TOKENS: usize = 0;

/// An instance of an epoch scheduler, typically one per CPU.
pub struct Scheduler {
    idle_task: TaskRef,
    active: RunQueue,
    expired: RunQueue,
}

impl Scheduler {
    /// Creates a new epoch scheduler instance with the given idle task.
    pub const fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            active: RunQueue::new(),
            expired: RunQueue::new(),
        }
    }

    fn apply<F, R>(&mut self, f: F) -> R
    where
        F: Fn(&mut RunQueue) -> R,
        R: Returnable,
    {
        let (first, second) = if self.active.len() >= self.expired.len() {
            (&mut self.active, &mut self.expired)
        } else {
            (&mut self.expired, &mut self.active)
        };

        let first_result = f(first);

        if first_result.should_return() {
            first_result
        } else {
            f(second)
        }
    }
}

trait Returnable {
    fn should_return(&self) -> bool;
}

impl Returnable for bool {
    fn should_return(&self) -> bool {
        *self
    }
}

impl<T> Returnable for Option<T> {
    fn should_return(&self) -> bool {
        self.is_some()
    }
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        if self.active.is_empty() {
            if !self.expired.is_empty() {
                mem::swap(&mut self.active, &mut self.expired);
            } else {
                return self.idle_task.clone();
            }
        }
        self.active
            .next(&mut self.expired)
            .unwrap_or(self.idle_task.clone())
    }

    fn add(&mut self, task: TaskRef) {
        let task = EpochTaskRef::new(task);
        self.expired.push(task, DEFAULT_PRIORITY);
    }

    fn busyness(&self) -> usize {
        self.active.len() + self.expired.len()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        self.apply(|run_queue| run_queue.remove(task))
    }

    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler::PriorityScheduler> {
        Some(self)
    }

    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        let mut active = RunQueue::new();
        let mut expired = RunQueue::new();

        mem::swap(&mut self.active, &mut active);
        mem::swap(&mut self.expired, &mut expired);

        Box::new(active.drain().chain(expired.drain()))
    }

    fn tasks(&self) -> Vec<TaskRef> {
        self.active
            .clone()
            .drain()
            .chain(self.expired.clone().drain())
            .collect()
    }
}

impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let priority = core::cmp::min(priority, MAX_PRIORITY);
        self.apply(|run_queue| run_queue.set_priority(task, priority))
    }

    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        self.apply(|run_queue| run_queue.priority(task))
    }
}

#[derive(Debug, Clone)]
struct EpochTaskRef {
    task: TaskRef,
    tokens: usize,
}

impl Deref for EpochTaskRef {
    type Target = TaskRef;

    fn deref(&self) -> &TaskRef {
        &self.task
    }
}

impl DerefMut for EpochTaskRef {
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.task
    }
}

impl EpochTaskRef {
    fn new(task: TaskRef) -> EpochTaskRef {
        EpochTaskRef {
            task,
            tokens: INITIAL_TOKENS,
        }
    }
}

impl From<EpochTaskRef> for TaskRef {
    fn from(value: EpochTaskRef) -> Self {
        value.task
    }
}
