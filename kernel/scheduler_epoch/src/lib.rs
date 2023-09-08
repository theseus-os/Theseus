//! This crate picks the next task on token based scheduling policy.
//! At the begining of each scheduling epoch a set of tokens is distributed
//! among tasks depending on their priority.
//! [tokens assigned to each task = (prioirty of each task / prioirty of all
//! tasks) * length of epoch]. Each time a task is picked, the token count of
//! the task is decremented by 1. A task is executed only if it has tokens
//! remaining. When all tokens of all runnable task are exhausted a new
//! scheduling epoch is initiated. In addition this crate offers the interfaces
//! to set and get priorities  of each task.

#![no_std]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::ops::{Deref, DerefMut};

use task::TaskRef;

const MAX_PRIORITY: u8 = 40;
const DEFAULT_PRIORITY: u8 = 20;
const INITIAL_TOKENS: usize = 10;

pub struct Scheduler {
    idle_task: TaskRef,
    queue: VecDeque<EpochTaskRef>,
}

impl Scheduler {
    pub const fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            queue: VecDeque::new(),
        }
    }

    /// Moves the `TaskRef` at the given index in this `RunQueue` to the end
    /// (back) of this `RunQueue`, and returns a cloned reference to that
    /// `TaskRef`. The number of tokens is reduced by one and number of context
    /// switches is increased by one. This function is used when the task is
    /// selected by the scheduler
    fn update_and_move_to_end(&mut self, index: usize, tokens: usize) -> Option<TaskRef> {
        if let Some(mut priority_task_ref) = self.queue.remove(index) {
            priority_task_ref.tokens_remaining = tokens;
            let task_ref = priority_task_ref.task.clone();
            self.queue.push_back(priority_task_ref);
            Some(task_ref)
        } else {
            None
        }
    }

    fn try_next(&mut self) -> Option<TaskRef> {
        if let Some((task_index, _)) = self
            .queue
            .iter()
            .enumerate()
            .find(|(_, task)| task.is_runnable() && task.tokens_remaining > 0)
        {
            let chosen_task = self.queue.get(task_index).unwrap();
            let modified_tokens = chosen_task.tokens_remaining.saturating_sub(1);
            self.update_and_move_to_end(task_index, modified_tokens)
        } else {
            None
        }
    }

    fn assign_tokens(&mut self) {
        // We begin with total priorities = 1 to avoid division by zero
        let mut total_priorities: usize = 1;

        // This loop calculates the total priorities of the runqueue
        for (_i, t) in self.queue.iter().enumerate() {
            // we skip the idle task, it contains zero tokens as it is picked last
            if t.is_an_idle_task {
                continue;
            }

            // we assign tokens only to runnable tasks
            if !t.is_runnable() {
                continue;
            }

            total_priorities = total_priorities
                .saturating_add(1)
                .saturating_add(t.priority as usize);
        }

        // We keep each epoch for 100 tokens by default
        // However since this granularity could miss low priority tasks when
        // many concurrent tasks are running, we increase the epoch in such cases
        let epoch: usize = core::cmp::max(total_priorities, 100);

        // We iterate through each task in runqueue
        // We dont use iterator as items are modified in the process
        for (_i, t) in self.queue.iter_mut().enumerate() {
            // we give zero tokens to the idle tasks
            if t.is_an_idle_task {
                continue;
            }

            // we give zero tokens to none runnable tasks
            if !t.is_runnable() {
                continue;
            }

            // task_tokens = epoch * (taskref + 1) / total_priorities;
            let task_tokens = epoch
                .saturating_mul((t.priority as usize).saturating_add(1))
                .wrapping_div(total_priorities);

            t.tokens_remaining = task_tokens;
            // debug!("assign_tokens(): AP {} chose Task {:?}", apic_id, &*t);
            // break;
        }
    }
}

impl task::scheduler::Scheduler for Scheduler {
    fn next(&mut self) -> TaskRef {
        self.try_next()
            .or_else(|| {
                self.assign_tokens();
                self.try_next()
            })
            .unwrap_or(self.idle_task.clone())
    }

    fn add(&mut self, task: TaskRef) {
        let priority_task_ref = EpochTaskRef::new(task);
        self.queue.push_back(priority_task_ref);
    }

    fn busyness(&self) -> usize {
        self.queue.len()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        let mut task_index = None;
        for (i, t) in self.queue.iter().enumerate() {
            if **t == *task {
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
        Some(self)
    }

    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        Box::new(self.queue.drain(..).map(|epoch_task| epoch_task.task))
    }

    fn dump(&self) -> Vec<TaskRef> {
        self.queue
            .clone()
            .into_iter()
            .map(|epoch_task| epoch_task.task)
            .collect()
    }
}

impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, mut priority: u8) -> bool {
        priority = core::cmp::min(priority, MAX_PRIORITY);

        for epoch_task in self.queue.iter_mut() {
            if epoch_task.task == *task {
                epoch_task.priority = priority;
                return true;
            }
        }
        false
    }

    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        for epoch_task in self.queue.iter() {
            if epoch_task.task == *task {
                return Some(epoch_task.priority);
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
struct EpochTaskRef {
    task: TaskRef,
    priority: u8,
    tokens_remaining: usize,
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
            priority: DEFAULT_PRIORITY,
            tokens_remaining: INITIAL_TOKENS,
        }
    }
}
