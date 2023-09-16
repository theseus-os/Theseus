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
#![feature(core_intrinsics)]

extern crate alloc;

use alloc::{boxed::Box, collections::VecDeque, vec::Vec};
use core::{
    cmp::max,
    intrinsics::unlikely,
    ops::{Deref, DerefMut},
    sync::atomic::Ordering,
};

use task::TaskRef;

const MAX_PRIORITY: u8 = 40;
const DEFAULT_PRIORITY: u8 = 20;
const INITIAL_TOKENS: usize = 10;

pub struct Scheduler {
    idle_task: TaskRef,
    have_tokens: VecDeque<EpochTaskRef>,
    out_of_tokens: Vec<EpochTaskRef>,
}

impl Scheduler {
    pub const fn new(idle_task: TaskRef) -> Self {
        Self {
            idle_task,
            have_tokens: VecDeque::new(),
            out_of_tokens: Vec::new(),
        }
    }

    // /// Moves the `TaskRef` at the given index in this `RunQueue` to the end
    // /// (back) of this `RunQueue`, and returns a cloned reference to that
    // /// `TaskRef`. The number of tokens is reduced by one and number of context
    // /// switches is increased by one. This function is used when the task is
    // /// selected by the scheduler
    // fn update_and_move_to_end(&mut self, index: usize, tokens: usize) ->
    // Option<TaskRef> {     if let Some(mut priority_task_ref) =
    // self.queue.remove(index) {         priority_task_ref.tokens_remaining =
    // tokens;         let task_ref = priority_task_ref.task.clone();
    //         self.queue.push_back(priority_task_ref);
    //         Some(task_ref)
    //     } else {
    //         None
    //     }
    // }

    fn try_next(&mut self) -> Option<TaskRef> {
        while let Some(task) = self.have_tokens.pop_front() {
            if task.task.is_runnable() {
                if let Some(task) = self.add_epoch_task(task) {
                    return Some(task);
                }
            } else {
                task.task
                    .expose_is_on_run_queue()
                    .store(false, Ordering::Release);
                // Checking this prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks is_on_run_queue so we have to do the opposite.
                //
                // TODO: This could be a relaxed load followed by a fence in the if statement.
                if unlikely(task.task.is_runnable()) {
                    if let Some(task) = self.add_epoch_task(task) {
                        return Some(task);
                    }
                }
            }
        }
        None
    }

    fn add_epoch_task(&mut self, mut task: EpochTaskRef) -> Option<TaskRef> {
        task.task
            .expose_is_on_run_queue()
            .store(true, Ordering::Release);
        match task.tokens_remaining.checked_sub(1) {
            Some(new_tokens) => {
                task.tokens_remaining = new_tokens;
                let task_ref = task.task.clone();
                self.have_tokens.push_back(task);
                Some(task_ref)
            }
            None => {
                self.out_of_tokens.push(task);
                None
            }
        }
    }

    fn assign_tokens(&mut self) {
        while let Some(task) = self.out_of_tokens.pop() {
            self.have_tokens.push_back(task);
        }

        let mut total_priorities = 1;
        for task in self.have_tokens.iter() {
            total_priorities += 1 + task.priority as usize;
        }

        let epoch = max(total_priorities, 100);

        for task in self.have_tokens.iter_mut() {
            task.tokens_remaining = epoch
                .saturating_mul((task.priority as usize).saturating_add(1))
                .wrapping_div(total_priorities);
        }
    }

    fn len(&self) -> usize {
        self.have_tokens.len() + self.out_of_tokens.len()
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
        let epoch_task_ref = EpochTaskRef::new(task);
        self.add_epoch_task(epoch_task_ref);
    }

    fn busyness(&self) -> usize {
        self.len()
    }

    fn remove(&mut self, task: &TaskRef) -> bool {
        let old_len = self.len();
        self.have_tokens
            .retain(|other_task| other_task.task != *task);
        self.out_of_tokens
            .retain(|other_task| other_task.task != *task);
        let new_len = self.len();
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

    fn drain(&mut self) -> Box<dyn Iterator<Item = TaskRef> + '_> {
        Box::new(
            self.have_tokens
                .drain(..)
                .chain(self.out_of_tokens.drain(..))
                .map(|epoch_task| epoch_task.task),
        )
    }

    fn dump(&self) -> Vec<TaskRef> {
        self.have_tokens
            .clone()
            .into_iter()
            .chain(self.out_of_tokens.clone())
            .map(|epoch_task| epoch_task.task)
            .collect()
    }
}

impl task::scheduler::PriorityScheduler for Scheduler {
    fn set_priority(&mut self, task: &TaskRef, mut priority: u8) -> bool {
        priority = core::cmp::min(priority, MAX_PRIORITY);

        for epoch_task in self
            .have_tokens
            .iter_mut()
            .chain(self.out_of_tokens.iter_mut())
        {
            if epoch_task.task == *task {
                epoch_task.priority = priority;
                return true;
            }
        }
        false
    }

    fn priority(&mut self, task: &TaskRef) -> Option<u8> {
        for epoch_task in self.have_tokens.iter().chain(self.out_of_tokens.iter()) {
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
