//! A token-based epoch scheduler.
//!
//! The implementation is based on the [`O(1)` Linux
//! scheduler][linux-scheduler].
//!
//! The scheduler is comprised of two run queues: an .
//!
//! Note that our implementation is not constant-time since we store
//! non-runnable tasks on the run queue.
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
    time::Duration,
};

use task::TaskRef;

use crate::queue::RunQueue;

const MAX_PRIORITY: u8 = 63;
const DEFAULT_PRIORITY: u8 = 20;

/// The minimum amount of time for every runnable task to run.
///
/// This is not strictly adhered to when the tasks are run
const TARGET_LATENCY: Duration = Duration::from_millis(15);

/// An epoch scheduler.
///
/// See crate-level docs for more information.
pub struct Scheduler {
    idle_task: TaskRef,
    active: RunQueue,
    expired: RunQueue,
    total_weight: usize,
}

impl Scheduler {
    /// Creates a new epoch scheduler instance with the given idle task.
    pub const fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            active: RunQueue::new(),
            expired: RunQueue::new(),
            // TODO: 0 or 1
            total_weight: 0,
        }
    }

    fn apply<F, R>(&mut self, mut f: F) -> R
    where
        F: FnMut(&mut RunQueue) -> R,
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
    #[inline]
    fn next(&mut self) -> TaskRef {
        if self.active.is_empty() {
            if !self.expired.is_empty() {
                mem::swap(&mut self.active, &mut self.expired);
            } else {
                return self.idle_task.clone();
            }
        }
        self.active
            .next(&mut self.expired, self.total_weight)
            .unwrap_or(self.idle_task.clone())
    }

    #[inline]
    fn add(&mut self, task: TaskRef) {
        let weight = weight(DEFAULT_PRIORITY);
        self.total_weight += weight;

        let task = EpochTaskRef::new(
            task,
            TaskConfiguration {
                weight,
                total_weight: self.total_weight,
            },
        );
        self.expired.push(task, DEFAULT_PRIORITY);
    }

    #[inline]
    fn busyness(&self) -> usize {
        self.active.len() + self.expired.len()
    }

    #[inline]
    fn remove(&mut self, task: &TaskRef) -> bool {
        match self.apply(|run_queue| run_queue.remove(task)) {
            Some(weight) => {
                self.total_weight -= weight;
                true
            }
            None => false,
        }
    }

    #[inline]
    fn as_priority_scheduler(&mut self) -> Option<&mut dyn task::scheduler::PriorityScheduler> {
        Some(self)
    }

    #[inline]
    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        let mut active = RunQueue::new();
        let mut expired = RunQueue::new();

        mem::swap(&mut self.active, &mut active);
        mem::swap(&mut self.expired, &mut expired);
        self.total_weight = 0;

        Box::new(active.drain().chain(expired.drain()))
    }

    #[inline]
    fn tasks(&self) -> Vec<TaskRef> {
        self.active
            .clone()
            .drain()
            .chain(self.expired.clone().drain())
            .collect()
    }
}

impl task::scheduler::PriorityScheduler for Scheduler {
    #[inline]
    fn set_priority(&mut self, task: &TaskRef, priority: u8) -> bool {
        let priority = core::cmp::min(priority, MAX_PRIORITY);
        self.apply(|run_queue| run_queue.set_priority(task, priority))
    }

    #[inline]
    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        self.apply(|run_queue| run_queue.priority(task))
    }
}

#[inline]
fn weight(priority: u8) -> usize {
    priority as usize + 1
}

#[derive(Debug, Clone)]
struct EpochTaskRef {
    task: TaskRef,
    tokens: usize,
}

impl EpochTaskRef {
    /// Creates a new task.
    ///
    /// Returns the task and the weight of the task.
    #[must_use]
    pub(crate) fn new(task: TaskRef, config: TaskConfiguration) -> Self {
        let mut task = Self { task, tokens: 0 };
        task.recalculate_tokens(config);
        task
    }

    #[inline]
    pub(crate) fn recalculate_tokens(&mut self, config: TaskConfiguration) {
        const TOTAL_TOKENS: usize = TARGET_LATENCY.as_micros() as usize
            / kernel_config::time::CONFIG_TIMESLICE_PERIOD_MICROSECONDS as usize;

        // TODO
        self.tokens = core::cmp::max(TOTAL_TOKENS * config.weight / config.total_weight, 1);
    }
}

pub(crate) struct TaskConfiguration {
    /// The weight of the task.
    pub(crate) weight: usize,
    /// The sum of the weights of all tasks on the run queue.
    pub(crate) total_weight: usize,
}

impl Deref for EpochTaskRef {
    type Target = TaskRef;

    #[inline]
    fn deref(&self) -> &TaskRef {
        &self.task
    }
}

impl DerefMut for EpochTaskRef {
    #[inline]
    fn deref_mut(&mut self) -> &mut TaskRef {
        &mut self.task
    }
}

impl From<EpochTaskRef> for TaskRef {
    #[inline]
    fn from(value: EpochTaskRef) -> Self {
        value.task
    }
}
