#![no_std]

extern crate alloc;

use alloc::collections::VecDeque;
use core::ops::{self, Deref};
use task::TaskRef;

pub struct RunQueue<T> {
    core: u8,
    queue: VecDeque<T>,
}

impl<T> ops::Deref for RunQueue<T> {
    type Target = VecDeque<T>;

    fn deref(&self) -> &Self::Target {
        &self.queue
    }
}

impl<T> ops::DerefMut for RunQueue<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.queue
    }
}

impl<T> RunQueue<T> {
    pub fn new(core: u8) -> Self {
        Self {
            core,
            queue: VecDeque::new(),
        }
    }

    pub fn push_back(&mut self, task: T) {
        self.queue.push_back(task);
        // FIXME: SIMD
    }
}

impl<T> RunQueue<T>
where
    T: Deref<Target = TaskRef>,
{
    #[cfg(runqueue_spillful)]
    pub fn remove_task(&mut self, _: &TaskRef) {
        // For the runqueue state spill evaluation, we disable this method
        // because we only want to allow removing a task from a runqueue
        // from within the TaskRef::internal_exit() method.
    }

    #[cfg(not(runqueue_spillful))]
    pub fn remove_task(&mut self, task: &TaskRef) {
        self.queue.retain(|x| x.deref() != task)
        // FIXME: SIMD
    }
}

impl<T> RunQueue<T>
where
    T: Clone,
{
    pub fn move_to_end(&mut self, index: usize) -> Option<T> {
        self.swap_remove_front(index).map(|task| {
            self.push_back(task.clone());
            task
        })
    }
}
