use crate::TaskRef;
use alloc::collections::VecDeque;
use core::{intrinsics::unlikely, sync::atomic::Ordering};

#[derive(Debug)]
pub(crate) struct Queue {
    inner: VecDeque<TaskRef>,
}

impl Queue {
    pub(crate) fn new() -> Self {
        Self {
            inner: VecDeque::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(crate) fn add(&mut self, task: TaskRef) {
        self.inner.push_back(task)
    }

    pub(crate) fn remove(&mut self, task: &TaskRef) {
        self.inner.retain(|other_task| other_task != task);
    }

    pub(crate) fn next(&mut self) -> Option<TaskRef> {
        while let Some(task) = self.inner.pop_front() {
            if task.is_runnable() {
                self.inner.push_back(task.clone());
                return Some(task);
            } else {
                task.is_on_run_queue.store(false, Ordering::Release);
                // Checking this prevents an interleaving where `TaskRef::unblock` wouldn't add
                // the task back onto the run queue. `TaskRef::unblock` sets the run state and
                // then checks is_on_run_queue so we have to do the opposite.
                //
                // TODO: This could be a relaxed load followed by a fence in the if statement.
                if unlikely(task.is_runnable()) {
                    task.is_on_run_queue.store(true, Ordering::Release);
                    self.inner.push_back(task.clone());
                    return Some(task);
                }
            }
        }
        None
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &TaskRef> {
        self.inner.iter()
    }

    pub(crate) fn get_priority(&self, _: &TaskRef) -> Option<u8> {
        None
    }

    pub(crate) fn set_priority(&mut self, _: &TaskRef, _: u8) -> Result<(), &'static str> {
        Err("cannot set priority using round robin scheduler")
    }

    pub(crate) fn set_periodicity(&mut self, _: &TaskRef, _: usize) -> Result<(), &'static str> {
        Err("cannot set periodicity using round robin scheduler")
    }
}
